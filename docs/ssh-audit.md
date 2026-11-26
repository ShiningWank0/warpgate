# SSH audit correlation

Warpgate sends two environment variables on each target SSH session channel
before starting a shell, exec command, or subsystem. The values are generated
by Warpgate. The SSH proxy ignores client requests to set either name.

| Environment variable | Correlation field |
| --- | --- |
| `WARPGATE_SESSION_ID` | Warpgate audit field `session`, the user-session UUID |
| `WARPGATE_CHANNEL_ID` | Warpgate audit field `channel_id`, the channel UUID |

Configure the target OpenSSH server to accept the two variables. Scope the
listener or target account to connections from Warpgate and restrict access
to the gateway's SSH identity.

```sshconfig
AcceptEnv WARPGATE_SESSION_ID WARPGATE_CHANNEL_ID
```

Use these values in an entry recorded by your target's trusted SSH audit
configuration before handing control to the user. For example, an
administrator can use the sshd log of environment requests at `LogLevel
DEBUG2` to associate the IDs with the sshd process and its connection tuple.
Verify the messages for the installed OpenSSH version and restrict access to
these logs. Debug logging can also contain other environment values. In
production use the narrowest trusted collection available on the target.

Within Warpgate, the `SshSessionLinked1` audit event maps the user-session
UUID to the `target_session_id` used by its recordings. The
`SshExecRequested1` event records the original exec request.
`SshShellCommandObserved1` is an inference from the visible terminal stream.
For exact process execution and file access, collect target OS audit events
and join them through the trusted session-start evidence. Treat environment
values read from an already-running user shell as user-controlled.

This channel metadata applies to shell, exec and subsystem requests. SSH port
forwarding channels have no session environment. Warpgate's SSH traffic
recordings associate those channels with their target session independently.
