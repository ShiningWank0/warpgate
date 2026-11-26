import os
from pathlib import Path
from uuid import uuid4

from .api_client import admin_client, sdk
from .conftest import ProcessManager, WarpgateProcess
from .util import wait_port


def _create_user_and_target(api, ssh_port):
    role = api.create_role(
        sdk.RoleDataRequest(name=f"role-{uuid4()}"),
    )
    user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
    api.add_user_role(user.id, role.id)
    ssh_target = api.create_target(
        sdk.TargetDataRequest(
            name=f"ssh-{uuid4()}",
            require_approval=False,
            ticket_requests_disabled=False,
            ticket_require_approval=False,
            options=sdk.TargetOptions(
                sdk.TargetOptionsTargetSSHOptions(
                    kind="Ssh",
                    allow_insecure_algos=False,
                    host="localhost",
                    port=ssh_port,
                    username="root",
                    auth=sdk.SSHTargetAuth(
                        sdk.SSHTargetAuthSshTargetPublicKeyAuth(kind="PublicKey")
                    ),
                )
            ),
        )
    )
    api.add_target_role(ssh_target.id, role.id)
    return user, ssh_target


def _uses_left(api, ticket_id):
    for ticket in api.get_tickets():
        if ticket.id == ticket_id:
            return ticket.uses_left
    return None


class Test:
    def test(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        timeout,
        shared_wg: WarpgateProcess,
    ):
        ssh_port = processes.start_ssh_server(
            trusted_keys=[wg_c_ed25519_pubkey.read_text()]
        )

        wait_port(ssh_port)

        url = f"https://localhost:{shared_wg.http_port}"
        with admin_client(url) as api:
            user, ssh_target = _create_user_and_target(api, ssh_port)
            api.create_password_credential(
                user.id, sdk.NewPasswordCredential(password="123")
            )

            secret = api.create_ticket(
                sdk.CreateTicketRequest(
                    target_name=ssh_target.name,
                    username=user.username,
                )
            ).secret

            ssh_client = processes.start_ssh_client(
                f"ticket-{secret}@localhost",
                "-p",
                str(shared_wg.ssh_port),
                "-i",
                "/dev/null",
                "-o",
                "PreferredAuthentications=password",
                "ls",
                "/bin/sh",
                password="123",
            )
            assert ssh_client.communicate(timeout=timeout)[0] == b"/bin/sh\n"
            assert ssh_client.returncode == 0

    def test_publickey_offer_spends_ticket_exactly_once(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        timeout,
        shared_wg: WarpgateProcess,
    ):
        ssh_port = processes.start_ssh_server(
            trusted_keys=[wg_c_ed25519_pubkey.read_text()]
        )

        wait_port(ssh_port)

        url = f"https://localhost:{shared_wg.http_port}"
        with admin_client(url) as api:
            user, ssh_target = _create_user_and_target(api, ssh_port)

            created = api.create_ticket(
                sdk.CreateTicketRequest(
                    target_name=ssh_target.name,
                    username=user.username,
                    number_of_uses=1,
                )
            )
            ticket_id, secret = created.ticket.id, created.secret
            assert _uses_left(api, ticket_id) == 1

            # Public half only and no agent: the offer gets PK_OK but the client
            # has nothing to sign the follow-up with. The final check alone can't
            # catch an offer that spends the ticket, since the signed request
            # would then hit the session's cached ticket auth.
            offer_only_env = {**os.environ}
            offer_only_env.pop("SSH_AUTH_SOCK", None)
            offer_only_client = processes.start_ssh_client(
                f"ticket-{secret}@localhost",
                "-p",
                str(shared_wg.ssh_port),
                "-i",
                "ssh-keys/id_ed25519.pub",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "PreferredAuthentications=publickey",
                "ls",
                "/bin/sh",
                env=offer_only_env,
            )
            offer_only_client.communicate(timeout=timeout)
            assert offer_only_client.returncode != 0
            assert _uses_left(api, ticket_id) == 1

            ssh_client = processes.start_ssh_client(
                f"ticket-{secret}@localhost",
                "-p",
                str(shared_wg.ssh_port),
                "-i",
                "ssh-keys/id_ed25519",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "PreferredAuthentications=publickey",
                "ls",
                "/bin/sh",
            )
            assert ssh_client.communicate(timeout=timeout)[0] == b"/bin/sh\n"
            assert ssh_client.returncode == 0
            assert _uses_left(api, ticket_id) == 0


def test_short_lived_ticket_policy(processes, wg_c_ed25519_pubkey, timeout):
    from datetime import datetime, timedelta, timezone

    wg = processes.start_wg(
        config_patch={
            "ssh": {
                "session_open_timeout": "1m",
                "session_max_age": "1m",
                "ticket_max_ttl": "5m",
            }
        }
    )
    ssh_port = processes.start_ssh_server(
        trusted_keys=[wg_c_ed25519_pubkey.read_text()]
    )
    wait_port(wg.http_port, recv=False)
    wait_port(wg.ssh_port)
    wait_port(ssh_port)

    with admin_client(f"https://localhost:{wg.http_port}") as api:
        user, target = _create_user_and_target(api, ssh_port)
        api.create_password_credential(
            user.id, sdk.NewPasswordCredential(password="123")
        )
        short_ticket = api.create_ticket(
            sdk.CreateTicketRequest(
                target_name=target.name,
                username=user.username,
                number_of_uses=1,
                expiry=datetime.now(timezone.utc) + timedelta(minutes=2),
            )
        )
        unlimited_ticket = api.create_ticket(
            sdk.CreateTicketRequest(target_name=target.name, username=user.username)
        )

    def connect(selector):
        client = processes.start_ssh_client(
            f"{selector}@localhost",
            "-p",
            str(wg.ssh_port),
            "-o",
            "PreferredAuthentications=password",
            "echo",
            "short-lived",
            password="123",
        )
        output, _ = client.communicate(timeout=timeout)
        return client.returncode, output

    assert connect(f"{user.username}:{target.name}")[0] != 0
    assert connect(f"ticket-{unlimited_ticket.secret}")[0] != 0
    assert connect(f"ticket-{short_ticket.secret}") == (0, b"short-lived\n")
    assert connect(f"ticket-{short_ticket.secret}")[0] != 0
