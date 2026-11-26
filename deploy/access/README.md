# Declarative Warpgate access

Use this OpenTofu or Terraform configuration from the protected repository
that owns user and role changes. Configure target and role definitions in
Warpgate's GitOps provider. Use the resulting role IDs when declaring user
assignments here.

```hcl
warpgate_url = "https://gateway.example.net"

users = {
  alice = {
    description = "Database operator"
  }
}

role_assignments = {
  alice-observer = {
    username = "alice"
    role_id  = "00000000-0000-0000-0000-000000000000"
  }
}
```

| Change | Review and application |
| --- | --- |
| Access for one SSH session | Request a ticket in Warpgate. After approval, the requester activates it and receives the short-lived credential. |
| Standing access | Review a change to a user's role assignments. Apply it after merging the approved change. Omit `expires_at` for permanent membership. |
| Access until a particular date | Use an explicit RFC 3339 `expires_at` in the reviewed assignment. When the date passes, Warpgate rejects access via the expired role and closes affected sessions. |

Provide `TF_VAR_admin_api_token` via the CI secret mechanism. The token is
ephemeral and excluded from plan and state. Use a restricted administrator
identity with permission to manage users, access role bindings and gateway
settings. Keep state in a restricted remote backend and serialize apply
jobs. Import an existing user before managing it with this configuration,
and declare any per-user credential policy or network limits it must keep.
Apply protects credentials by leaving credentials and secrets outside the
user object. Use Warpgate's enrollment or an identity provider for login.

```sh
tofu init
tofu plan
tofu apply
tofu output -raw warpgate_runtime_options
```

Merge the runtime output into the gateway's existing `warpgate.yaml`.
For the bundled chart, combine the keys with the rest of the
`overrides_config` value. Reload the gateway configuration and restart the
gateway when installing webhook environment variables. Configure the SSH
target host fingerprints in Warpgate before enabling ticket-only access.

The module enables MFA, single-use tickets, and the self-service approval
queue. Set `mfa_enforcement = "Enroll"` to begin web enrollment or
`"Require"` to require MFA for interactive logins. For SSO identities, set
`mfa_policy_exempt_sso_users = true` only when the configured identity
provider enforces MFA. The one-use SSH ticket is issued from an
authenticated Warpgate web session and can be redeemed without a private
key.

Optionally pass `recordings_s3` from the [audit archive](../audit/README.md)
deployment. Warpgate uses credentials from its workload identity or its
service environment. The webhook variables are described in
[Short-lived SSH access](../../docs/ssh-short-lived-access.md).

To import existing objects, use the object ID and protect state during the
operation.

```sh
tofu import 'restapi_object.user["alice"]' '<user-uuid>'
tofu import 'restapi_object.role_assignment["alice-observer"]' '<role-uuid>'
```
