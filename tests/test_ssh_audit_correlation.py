import os
from uuid import UUID

from .test_ssh_proto import common_args, setup_user_and_target
from .util import wait_port


def test_target_receives_gateway_correlation_ids(
    processes, timeout, wg_c_ed25519_pubkey, monkeypatch
):
    wg = processes.start_wg()
    wait_port(wg.http_port, recv=False)
    wait_port(wg.ssh_port)
    user, target = setup_user_and_target(
        processes,
        wg,
        wg_c_ed25519_pubkey,
        extra_config="AcceptEnv WARPGATE_SESSION_ID WARPGATE_CHANNEL_ID",
    )
    monkeypatch.setenv("WARPGATE_SESSION_ID", "client-supplied-session")
    monkeypatch.setenv("WARPGATE_CHANNEL_ID", "client-supplied-channel")
    client = processes.start_ssh_client(
        f"{user.username}:{target.name}@localhost",
        "-p",
        str(wg.ssh_port),
        *common_args,
        "-o",
        "SendEnv=WARPGATE_SESSION_ID WARPGATE_CHANNEL_ID",
        """printf 'correlation:%s:%s\\n' "$WARPGATE_SESSION_ID" "$WARPGATE_CHANNEL_ID" """,
        password="123",
        env=os.environ.copy(),
    )
    stdout, _ = client.communicate(timeout=timeout)
    assert client.returncode == 0
    line = next(line for line in stdout.splitlines() if line.startswith(b"correlation:"))
    _, session_id, channel_id = line.decode().split(":")
    assert UUID(session_id).version == 4
    assert UUID(channel_id).version == 4
    assert session_id != channel_id
