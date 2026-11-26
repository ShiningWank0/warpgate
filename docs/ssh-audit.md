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

When the target authenticates Warpgate with an SSH certificate, its existing
signed key ID has the form `warpgate:<username>:<session UUID>` and appears
in authoritative target authentication logs. Certificate authentication
also supports connection-level correlation for port-forward-only use.

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

## File transfers

OpenSSH `sftp` and current OpenSSH `scp` use the SFTP subsystem. For SFTP
version 3, Warpgate emits `SshSftpOperation1` events with the operation,
paths when available, open flags, byte offset, requested and returned byte
counts, and the server status code. Payload bytes are excluded from these
audit events. Each event has the same session and channel IDs used for
command auditing.

Watch for `SshSftpAuditGap1` when a stream exceeds the parser bounds or uses
an unsupported version. The proxy continues passing traffic in that case.
`SshSftpUnanswered1` indicates that the channel ended before all requests
received a response. The older `scp -O` mode and tools such as rsync use exec
channels. Use target OS file auditing for those tools, for server-side
extensions, and to cover transfers carried inside arbitrary shell commands
or SSH forwarding.
