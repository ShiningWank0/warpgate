output "recordings_s3" {
  description = "Warpgate S3 recording settings before adding the runtime credential reference."
  value = {
    bucket                 = var.bucket_name
    endpoint               = local.archive_endpoint == "" ? null : local.archive_endpoint
    region                 = local.archive_region
    path_style             = var.backend == "r2"
    prefix                 = "recordings"
    serve_through_warpgate = true
  }
}

output "vector_config" {
  description = "Vector collector configuration. Save with tofu output -raw vector_config."
  value = templatefile("${path.module}/vector.toml.tftpl", {
    bucket        = var.bucket_name
    endpoint      = local.archive_endpoint
    region        = local.archive_region
    loki          = var.loki_endpoint
    journal_units = var.journal_units
  })
}

output "s3_collector_policy" {
  description = "AWS IAM policy for the collector identity. Attach separately to the runtime role."
  value = var.backend == "s3" ? jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["s3:PutObject", "s3:AbortMultipartUpload"]
      Resource = "${aws_s3_bucket.audit[0].arn}/audit/*"
    }]
  }) : null
}
