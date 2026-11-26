terraform {
  required_version = ">= 1.11.0"

  required_providers {
    restapi = {
      source  = "Mastercard/restapi"
      version = "~> 3.0"
    }
  }
}

provider "restapi" {
  uri                   = "${trimsuffix(var.warpgate_url, "/")}/@warpgate/admin/api"
  create_returns_object = true
  headers = {
    "X-Warpgate-Token" = var.admin_api_token
  }
}

provider "restapi" {
  alias = "parameters"
  uri   = "${trimsuffix(var.warpgate_url, "/")}/@warpgate/admin/api"
  headers = {
    "X-Warpgate-Token" = var.admin_api_token
  }
}

resource "restapi_object" "user" {
  for_each = var.users

  path                    = "/users"
  ignore_server_additions = true
  data = jsonencode({
    username                    = each.key
    description                 = each.value.description
    credential_policy           = each.value.credential_policy
    allowed_ip_ranges           = each.value.allowed_ip_ranges
    rate_limit_bytes_per_second = each.value.rate_limit_bytes_per_second
  })
}

resource "restapi_object" "role_assignment" {
  for_each = var.role_assignments

  path                    = "/users/${restapi_object.user[each.value.username].id}/roles"
  create_path             = "/users/${restapi_object.user[each.value.username].id}/roles/${each.value.role_id}"
  object_id               = each.value.role_id
  ignore_server_additions = true
  data = jsonencode(merge(
    { expires_at = each.value.expires_at },
    each.value.expires_at == null ? { is_active = true } : {},
  ))
}

locals {
  recordings = merge({}, [for config in var.recordings_s3[*] : {
    recordings_enable = true
    recordings_storage = merge(config, {
      kind        = "S3"
      credentials = { mode = "Auto" }
    })
  }]...)
}

# The parameters object exists with every Warpgate installation. Removing
# this resource from state should never call an API to delete it.
resource "restapi_object" "gateway_policy" {
  provider                = restapi.parameters
  path                    = "/parameters"
  object_id               = "parameters"
  create_method           = "PUT"
  read_path               = "/parameters"
  update_path             = "/parameters"
  ignore_server_additions = true
  data = jsonencode(merge({
    mfa_enforcement                     = var.mfa_enforcement
    mfa_policy_exempt_sso_users         = var.mfa_policy_exempt_sso_users
    ticket_self_service_enabled         = true
    ticket_auto_approve_existing_access = true
    ticket_max_duration_seconds         = var.ticket_lifetime_seconds
    ticket_max_uses                     = 1
    ticket_require_description          = true
    ticket_request_show_all_targets     = var.show_targets_for_escalation
  }, local.recordings))

  lifecycle {
    prevent_destroy = true
  }
}

output "warpgate_runtime_options" {
  description = "Merge these keys into the existing warpgate.yaml or Helm overrides."
  value = yamlencode({
    ssh = {
      enable               = true
      ticket_max_ttl       = "${var.ticket_lifetime_seconds}s"
      session_open_timeout = var.session_open_timeout
      session_max_age      = var.session_max_age
    }
    log = { format = "json" }
  })
}
