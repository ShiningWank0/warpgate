import os
import socket
import tempfile
import threading
import time
import unittest
from pathlib import Path

import paramiko

from file_window import GatewayOnly, generate_host_key, open_source, serve


class FileWindowTests(unittest.TestCase):
    def test_source_must_be_a_regular_file_below_the_export_root(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "link").symlink_to("/etc/passwd")
            os.mkfifo(Path(root) / "fifo")
            for path in ("../secret", "/etc/passwd", "link", "fifo"):
                with self.subTest(path=path), self.assertRaises((ValueError, OSError)):
                    with open_source(root, path):
                        self.fail("unsafe path was opened")

    def test_sftp_exposes_only_one_file_and_closes_at_deadline(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "allowed.txt").write_bytes(b"allowed content")
            (Path(root) / "sibling.txt").write_bytes(b"hidden content")
            host = generate_host_key()
            gateway = generate_host_key()
            stop = threading.Event()
            with open_source(root, "allowed.txt") as (file, name), socket.create_server(
                ("127.0.0.1", 0)
            ) as listener:
                deadline = time.monotonic() + 2
                worker = threading.Thread(
                    target=serve,
                    args=(listener, host, GatewayOnly("temporary", gateway, deadline), file, name, stop),
                )
                worker.start()
                try:
                    with paramiko.Transport(listener.getsockname()) as ssh:
                        ssh.connect(hostkey=host, username="temporary", pkey=gateway)
                        with paramiko.SFTPClient.from_transport(ssh) as sftp:
                            self.assertEqual(sftp.listdir("."), ["allowed.txt"])
                            with sftp.open("/allowed.txt") as handle:
                                self.assertEqual(handle.read(), b"allowed content")
                            for path, mode in (
                                ("/allowed.txt", "w"),
                                ("/sibling.txt", "r"),
                                ("../allowed.txt", "r"),
                            ):
                                with self.subTest(path=path, mode=mode), self.assertRaises(OSError):
                                    sftp.open(path, mode)
                            with self.assertRaises(OSError):
                                sftp.remove("/allowed.txt")
                        with self.assertRaises(paramiko.SSHException):
                            ssh.open_session().exec_command("cat /sibling.txt")
                        worker.join(timeout=5)
                        self.assertFalse(worker.is_alive())
                        until = time.monotonic() + 2
                        while ssh.is_active() and time.monotonic() < until:
                            time.sleep(0.05)
                        self.assertFalse(ssh.is_active())
                finally:
                    stop.set()
                    worker.join(timeout=5)


if __name__ == "__main__":
    unittest.main()
