resource "aws_s3_bucket" "audit" {
  count               = var.backend == "s3" ? 1 : 0
  bucket              = var.bucket_name
  object_lock_enabled = true

  lifecycle {
    prevent_destroy = true
  }
}

resource "aws_s3_bucket_public_access_block" "audit" {
  count                   = length(aws_s3_bucket.audit)
  bucket                  = aws_s3_bucket.audit[count.index].id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_versioning" "audit" {
  count  = length(aws_s3_bucket.audit)
  bucket = aws_s3_bucket.audit[count.index].id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_object_lock_configuration" "audit" {
  count  = length(aws_s3_bucket.audit)
  bucket = aws_s3_bucket_versioning.audit[count.index].bucket

  rule {
    default_retention {
      mode = "COMPLIANCE"
      days = var.retention_days
    }
  }

  lifecycle {
    prevent_destroy = true
  }
}

resource "aws_s3_bucket_policy" "audit" {
  count  = length(aws_s3_bucket.audit)
  bucket = aws_s3_bucket.audit[count.index].id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid       = "RequireTLSForArchive"
      Effect    = "Deny"
      Principal = "*"
      Action    = "s3:*"
      Resource = [
        aws_s3_bucket.audit[count.index].arn,
        "${aws_s3_bucket.audit[count.index].arn}/*",
      ]
      Condition = { Bool = { "aws:SecureTransport" = "false" } }
    }]
  })

  depends_on = [aws_s3_bucket_public_access_block.audit]
}

resource "cloudflare_r2_bucket" "audit" {
  count      = var.backend == "r2" ? 1 : 0
  account_id = var.cloudflare_account_id
  name       = var.bucket_name

  lifecycle {
    prevent_destroy = true
    precondition {
      condition     = can(regex("^[0-9a-fA-F]{32}$", var.cloudflare_account_id))
      error_message = "Provide your Cloudflare account ID when using R2."
    }
  }
}

resource "cloudflare_r2_bucket_lock" "audit" {
  count       = length(cloudflare_r2_bucket.audit)
  account_id  = var.cloudflare_account_id
  bucket_name = cloudflare_r2_bucket.audit[count.index].name

  rules = [{
    id      = "warpgate-audit-retention"
    enabled = true
    prefix  = ""
    condition = {
      type            = "Age"
      max_age_seconds = var.retention_days * 86400
    }
  }]

  lifecycle {
    prevent_destroy = true
  }
}

locals {
  archive_endpoint = var.backend == "r2" ? "https://${var.cloudflare_account_id}.r2.cloudflarestorage.com" : ""
  archive_region   = var.backend == "r2" ? "auto" : var.aws_region
}
