# Single-file SFTP access

This experimental operator tool creates a temporary Warpgate user, SSH
target, pinned host key and one-use access ticket. It serves one opened
regular file over SFTP until the earlier of session close or expiry.

Run it as a dedicated unprivileged account on a trusted Linux host. That
account needs read permission to the export directory. Restrict the export
directory and its ancestors to a trusted owner. Use a private listener
reachable only from the gateway. The helper authenticates the gateway using
its existing public key and rejects SSH shell, exec, forwarding, file
writes, and access to sibling paths.

```sh
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
.venv/bin/python file_window.py \
  --warpgate-url https://gateway.example.net \
  --gateway-public-key /etc/warpgate/gateway-public-key.pub \
  --root /srv/approved-exports \
  --file report.csv \
  --reason 'Approved request reference' \
  --lifetime 300
```

Supply `WARPGATE_FILE_WINDOW_API_TOKEN` through the service's secret
mechanism. It must be allowed to manage the temporary user, target, host
key and tickets. Restrict who can invoke this tool. Use a CA trusted by the
host for the Warpgate admin endpoint and configure Warpgate to validate
known host keys. Choose a lifetime at or below the gateway's
`ticket_max_ttl`.

Keep the helper running and deliver the `ssh_username` from its standard
output to the intended reader through a private channel. The reader can
download the named file with OpenSSH `sftp` or modern `scp` through
Warpgate. The helper reads the selected file descriptor, so a pathname
rename or replacement does not switch it to a different file. If contents
must be fixed, select a completed export whose producer has closed it.

The deadline closes the backend SSH transport even during a transfer. The
helper removes its Warpgate records after normal exit. If the helper or
host is forcibly terminated, its ephemeral SSH host key and listener are
gone and the ticket still expires. Administrators should clean up any
remaining `file-window-*` user and target records and their known-host
entries after a crash. Preserve the gateway account and audit records for
the approved request.
