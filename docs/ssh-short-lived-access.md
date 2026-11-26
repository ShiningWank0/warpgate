# Short-lived SSH access

```yaml
ssh:
  enable: true
  session_open_timeout: 3m
  session_max_age: 30m
  ticket_max_ttl: 5m
```

| Setting | Starts | Expires |
| --- | --- | --- |
| Ticket expiry | When Warpgate issues the ticket | After the configured ticket lifetime or when its single use is spent |
| `session_open_timeout` | When the gateway accepts the incoming connection | Once authentication to the target finishes. Includes target selection, approval and host-key prompts |
| `session_max_age` | When authentication to the target finishes | When the limit elapses, including during commands and file transfers |

The example accepts only finite-lifetime tickets with one use remaining on
the native SSH listener. Web SSH is disabled by this policy so it cannot
open a separate connection under the user's persistent credentials. The
gateway authenticates to the target with its own configured identity. Users
receive the access ticket through Warpgate.

Enable self-service tickets, configure one use and a ticket duration at or
below the SSH limit, and enable the relevant targets for ticket requests.
A ticket for a target outside a user's roles needs an administrator's
approval. Warpgate's existing requested-ticket activation flow hands the
bearer credential to the signed-in user. Require MFA for gateway web sign-in
using Warpgate's TOTP policy or the SSO provider's MFA policy. Protect the
ticket as a secret until redemption and use the Warpgate host-key fingerprint
when connecting.

A forced SSH disconnect ends access through Warpgate. Processes deliberately
detached on the target can outlive the connection. Use the target's service
manager or session cgroup policy when detached processes must also end.
