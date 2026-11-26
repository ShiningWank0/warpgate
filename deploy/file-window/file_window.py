#!/usr/bin/env python3
"""Provision one read-only SFTP file through a Warpgate ticket."""

import argparse
import base64
import io
import json
import logging
import os
import signal
import socket
import stat
import threading
import time
from contextlib import ExitStack, contextmanager
from datetime import datetime, timezone
from pathlib import PurePosixPath
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener
from uuid import UUID, uuid4

import paramiko
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

LOGGER = logging.getLogger("warpgate-file-window")
MAX_READ = 256 * 1024


@contextmanager
def open_source(root, relative_path):
    path = PurePosixPath(relative_path)
    if path.is_absolute() or not path.parts or ".." in path.parts:
        raise ValueError("pass a relative file path inside the export root")

    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    with ExitStack() as stack:
        directory = os.open(root, directory_flags)
        stack.callback(os.close, directory)
        for component in path.parts[:-1]:
            directory = os.open(component, directory_flags, dir_fd=directory)
            stack.callback(os.close, directory)
        source = os.open(
            path.name,
            os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
            dir_fd=directory,
        )
        stack.callback(os.close, source)
        if not stat.S_ISREG(os.fstat(source).st_mode):
            raise ValueError("the selected path must be a regular file")
        yield source, path.name


class ReadHandle(paramiko.SFTPHandle):
    def __init__(self, source, deadline):
        super().__init__(os.O_RDONLY)
        self.source = os.dup(source)
        self.deadline = deadline

    def read(self, offset, length):
        if time.monotonic() >= self.deadline:
            return paramiko.SFTP_PERMISSION_DENIED
        try:
            return os.pread(self.source, min(MAX_READ, length), offset)
        except (OSError, OverflowError):
            return paramiko.SFTP_FAILURE

    def stat(self):
        if time.monotonic() >= self.deadline:
            return paramiko.SFTP_PERMISSION_DENIED
        attributes = paramiko.SFTPAttributes()
        attributes.st_size = os.fstat(self.source).st_size
        attributes.st_mode = stat.S_IFREG | 0o444
        return attributes

    def close(self):
        if self.source is not None:
            os.close(self.source)
            self.source = None
        return paramiko.SFTP_OK


class SingleFileSftp(paramiko.SFTPServerInterface):
    def __init__(self, server, source, name, deadline):
        super().__init__(server)
        self.source = source
        self.name = name
        self.paths = {name, f"/{name}", f"./{name}"}
        self.deadline = deadline

    def _attributes(self):
        result = paramiko.SFTPAttributes()
        result.filename = self.name
        result.st_size = os.fstat(self.source).st_size
        result.st_mode = stat.S_IFREG | 0o444
        return result

    def list_folder(self, path):
        if time.monotonic() >= self.deadline:
            return paramiko.SFTP_PERMISSION_DENIED
        return [self._attributes()] if path in {"/", ".", ""} else paramiko.SFTP_NO_SUCH_FILE

    def stat(self, path):
        if time.monotonic() >= self.deadline:
            return paramiko.SFTP_PERMISSION_DENIED
        if path in self.paths:
            return self._attributes()
        if path in {"/", ".", ""}:
            result = paramiko.SFTPAttributes()
            result.st_mode = stat.S_IFDIR | 0o555
            return result
        return paramiko.SFTP_NO_SUCH_FILE

    lstat = stat

    def open(self, path, flags, attr):
        if time.monotonic() >= self.deadline or flags != os.O_RDONLY:
            return paramiko.SFTP_PERMISSION_DENIED
        if path not in self.paths:
            return paramiko.SFTP_NO_SUCH_FILE
        return ReadHandle(self.source, self.deadline)


class GatewayOnly(paramiko.ServerInterface):
    def __init__(self, username, public_key, deadline):
        self.username = username
        self.public_key = public_key
        self.deadline = deadline

    def get_allowed_auths(self, username):
        return "publickey"

    def check_auth_publickey(self, username, key):
        if username == self.username and key == self.public_key and time.monotonic() < self.deadline:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def check_channel_request(self, kind, channel_id):
        if kind == "session" and time.monotonic() < self.deadline:
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_env_request(self, channel, name, value):
        if name not in {b"WARPGATE_SESSION_ID", b"WARPGATE_CHANNEL_ID"}:
            return False
        try:
            identifier = UUID(value.decode("ascii"))
        except (ValueError, UnicodeError):
            return False
        LOGGER.info("%s=%s", name.decode("ascii"), identifier)
        return True


class NoRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, response, status, message, headers, new_url):
        return None


class WarpgateApi:
    def __init__(self, origin, token):
        url = urlsplit(origin)
        if (
            url.scheme != "https"
            or not url.hostname
            or url.username
            or url.password
            or url.path not in ("", "/")
            or url.query
            or url.fragment
        ):
            raise ValueError("Warpgate URL must be its HTTPS origin")
        self.base = origin.rstrip("/") + "/@warpgate/admin/api"
        self.opener = build_opener(NoRedirects())
        self.token = token

    def call(self, method, path, data=None):
        request = Request(
            self.base + path,
            data=json.dumps(data).encode() if data is not None else None,
            method=method,
            headers={
                "Content-Type": "application/json",
                "X-Warpgate-Token": self.token,
            },
        )
        try:
            with self.opener.open(request, timeout=8) as response:
                body = response.read(1024 * 1024)
                return json.loads(body) if body else None
        except HTTPError as error:
            if method == "DELETE" and error.code == 404:
                return None
            raise RuntimeError(f"Warpgate API {method} {path} returned {error.code}") from None


def load_public_key(path):
    with open(path, encoding="ascii") as file:
        key_type, encoded, *_ = file.read().split()
    return paramiko.PKey.from_type_string(key_type, base64.b64decode(encoded, validate=True))


def generate_host_key():
    private_key = Ed25519PrivateKey.generate().private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.OpenSSH,
        serialization.NoEncryption(),
    )
    return paramiko.Ed25519Key.from_private_key(io.StringIO(private_key.decode("ascii")))


def serve(listener, host_key, server, source, name, stop):
    deadline = server.deadline
    listener.settimeout(0.2)
    while time.monotonic() < deadline and not stop.is_set():
        try:
            connection, _ = listener.accept()
        except TimeoutError:
            continue
        with connection, paramiko.Transport(connection) as transport:
            transport.add_server_key(host_key)
            transport.set_subsystem_handler(
                "sftp", paramiko.SFTPServer, SingleFileSftp, source, name, deadline
            )
            transport.start_server(event=threading.Event(), server=server)
            handshake_deadline = min(deadline, time.monotonic() + 5)
            while transport.is_active() and not transport.is_authenticated():
                if time.monotonic() >= handshake_deadline or stop.wait(0.05):
                    break
            if transport.is_authenticated():
                while transport.is_active() and time.monotonic() < deadline:
                    if stop.wait(0.05):
                        break
                return


def provision(args, api, listener, host_key, username, expires, cleanup):
    port = listener.getsockname()[1]
    user = api.call("POST", "/users", {"username": username, "description": args.reason})
    cleanup.append(f"/users/{UUID(user['id'])}")
    known_host = api.call(
        "POST", "/ssh/known-hosts",
        {
            "host": args.target_host,
            "port": port,
            "key_type": host_key.get_name(),
            "key_base64": host_key.get_base64(),
        },
    )
    cleanup.append(f"/ssh/known-hosts/{UUID(known_host['id'])}")
    target = api.call(
        "POST", "/targets",
        {
            "name": username,
            "require_approval": False,
            "ticket_requests_disabled": True,
            "ticket_require_approval": False,
            "options": {
                "kind": "Ssh",
                "host": args.target_host,
                "port": port,
                "username": username,
                "allow_insecure_algos": False,
                "auth": {"kind": "PublicKey"},
            },
        },
    )
    cleanup.append(f"/targets/{UUID(target['id'])}")
    ticket = api.call(
        "POST", "/tickets",
        {
            "username": username,
            "target_name": target["name"],
            "number_of_uses": 1,
            "expiry": expires.isoformat(),
            "description": args.reason,
        },
    )
    return ticket["secret"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--warpgate-url", required=True)
    parser.add_argument("--gateway-public-key", required=True)
    parser.add_argument("--target-host", default="127.0.0.1",
                        help="private address Warpgate uses to reach this listener")
    parser.add_argument("--listen", default="127.0.0.1")
    parser.add_argument("--root", required=True, help="trusted source directory")
    parser.add_argument("--file", required=True, help="relative path in the source directory")
    parser.add_argument("--reason", required=True, help="approved request reference")
    parser.add_argument("--lifetime", type=int, default=300, help="seconds, at most 3600")
    args = parser.parse_args()
    if not 10 <= args.lifetime <= 3600:
        parser.error("--lifetime must be between 10 and 3600 seconds")
    api = WarpgateApi(args.warpgate_url, os.environ["WARPGATE_FILE_WINDOW_API_TOKEN"])
    key = load_public_key(args.gateway_public_key)
    host_key = generate_host_key()
    username = "file-window-" + uuid4().hex
    stop = threading.Event()
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda *_: stop.set())

    family = socket.AF_INET6 if ":" in args.listen else socket.AF_INET
    cleanup = []
    try:
        with open_source(args.root, args.file) as (source, name), socket.socket(family) as listener:
            listener.bind((args.listen, 0))
            listener.listen(4)
            deadline = time.monotonic() + args.lifetime
            expires = datetime.fromtimestamp(time.time() + args.lifetime, timezone.utc)
            LOGGER.info("Opening %s until %s", username, expires)
            secret = provision(args, api, listener, host_key, username, expires, cleanup)
            if stop.is_set() or time.monotonic() >= deadline:
                return
            print(json.dumps({"ssh_username": "ticket-" + secret, "file": name,
                              "expires_at": expires.isoformat()}), flush=True)
            serve(listener, host_key, GatewayOnly(username, key, deadline), source, name, stop)
    finally:
        for path in reversed(cleanup):
            try:
                api.call("DELETE", path)
            except (RuntimeError, OSError) as error:
                LOGGER.error("Manual cleanup required for %s (%s)", path, type(error).__name__)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    main()
