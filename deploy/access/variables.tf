variable "warpgate_url" {
  description = "HTTPS origin of the Warpgate control plane."
  type        = string

  validation {
    condition     = can(regex("^https://[^/?#@]+/?$", var.warpgate_url))
    error_message = "Use the HTTPS origin of Warpgate."
  }
}

variable "admin_api_token" {
  description = "Warpgate API token for the infrastructure service account."
  type        = string
  sensitive   = true
  ephemeral   = true
}

variable "users" {
  description = "Warpgate users, keyed by username. Manage authentication credentials separately."
  type = map(object({
    description                 = optional(string, "")
    credential_policy           = optional(map(list(string)))
    allowed_ip_ranges           = optional(list(string))
    rate_limit_bytes_per_second = optional(number)
  }))
  default = {}
}

variable "role_assignments" {
  description = "User access roles. Set an RFC3339 expiry for a temporary role. Omit expiry for a permanent role."
  type = map(object({
    username   = string
    role_id    = string
    expires_at = optional(string)
  }))
  default = {}
}

variable "mfa_enforcement" {
  description = "Warpgate MFA policy. Enroll prompts web users to set up TOTP. Require enforces MFA across interactive authentication."
  type        = string
  default     = "Require"

  validation {
    condition     = contains(["Enroll", "Require"], var.mfa_enforcement)
    error_message = "Use Enroll or Require."
  }
}

variable "mfa_policy_exempt_sso_users" {
  description = "Rely on the SSO provider's MFA policy for SSO users."
  type        = bool
  default     = false
}

variable "ticket_lifetime_seconds" {
  description = "Maximum time to redeem the one-use SSH ticket."
  type        = number
  default     = 300

  validation {
    condition     = var.ticket_lifetime_seconds >= 60 && var.ticket_lifetime_seconds <= 3600 && floor(var.ticket_lifetime_seconds) == var.ticket_lifetime_seconds
    error_message = "Set a whole number between 60 and 3600 seconds."
  }
}

variable "show_targets_for_escalation" {
  description = "Allow users to request a ticket to targets they could not otherwise access."
  type        = bool
  default     = true
}

variable "session_open_timeout" {
  description = "Warpgate duration for SSH opening, including approvals and target authentication."
  type        = string
  default     = "3m"
}

variable "session_max_age" {
  description = "Warpgate duration for the active SSH session."
  type        = string
  default     = "30m"
}

variable "recordings_s3" {
  description = "Optional output from the adjacent audit deployment. Storage credentials come from the Warpgate workload."
  type = object({
    bucket                 = string
    endpoint               = optional(string)
    region                 = string
    path_style             = bool
    prefix                 = string
    serve_through_warpgate = bool
  })
  default = null
}
