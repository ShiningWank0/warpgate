variable "backend" {
  description = "Object storage backend for the audit archive."
  type        = string
  default     = "s3"

  validation {
    condition     = contains(["s3", "r2"], var.backend)
    error_message = "Choose either s3 or r2."
  }
}

variable "bucket_name" {
  description = "Bucket dedicated to Warpgate recordings and audit archives."
  type        = string
}

variable "aws_region" {
  description = "AWS region when using the S3 backend."
  type        = string
  default     = "us-east-1"
}

variable "cloudflare_account_id" {
  description = "Cloudflare account ID when using R2."
  type        = string
  default     = ""
}

variable "retention_days" {
  description = "Number of days for which committed objects are protected from deletion."
  type        = number
  default     = 365

  validation {
    condition     = var.retention_days >= 1 && var.retention_days <= 36500 && floor(var.retention_days) == var.retention_days
    error_message = "Retention must be a whole number of days between 1 and 36500."
  }
}

variable "loki_endpoint" {
  description = "Base URL of a Loki instance or authenticated Loki reverse proxy. The empty string disables Loki forwarding."
  type        = string
  default     = ""
}

variable "journal_units" {
  description = "Systemd units from which this collector reads logs."
  type        = list(string)
  default     = ["warpgate.service", "ssh.service", "sshd.service"]
}
