import socket
import time

from .test_ssh_proto import common_args, setup_user_and_target
from .util import read_until, wait_port


def test_ssh_open_timeout_cuts_connection_before_authentication(processes):
    wg = processes.start_wg(
        config_patch={"ssh": {"session_open_timeout": "2s", "inactivity_timeout": "1m"}}
    )
    wait_port(wg.ssh_port)
    with socket.create_connection(("127.0.0.1", wg.ssh_port), timeout=8) as client:
        assert client.makefile("rb").readline().startswith(b"SSH-")
        # Leave key exchange pending. The opening deadline must end the socket.
        while client.recv(4096):
            pass


def test_ssh_session_max_age_cuts_active_connection(
    processes, timeout, wg_c_ed25519_pubkey
):
    wg = processes.start_wg(
        config_patch={"ssh": {"session_max_age": "3s", "inactivity_timeout": "1m"}}
    )
    wait_port(wg.http_port, recv=False)
    wait_port(wg.ssh_port)
    user, target = setup_user_and_target(processes, wg, wg_c_ed25519_pubkey)
    client = processes.start_ssh_client(
        f"{user.username}:{target.name}@localhost",
        "-p",
        str(wg.ssh_port),
        *common_args,
        "while true; do echo deadline-marker; sleep 0.1; done",
        password="123",
    )
    marker = b"deadline-marker"
    assert marker in read_until(client.stdout, marker, time.monotonic() + timeout)
    assert client.wait(timeout=10) != 0
