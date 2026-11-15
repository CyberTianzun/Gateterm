#!/usr/bin/env python3
"""Local Linux OCI smoke test. Requires root, runc, musl-gcc; never contacts production."""
import argparse
import base64
import http.client
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import uuid

HERE = Path(__file__).resolve().parent


def ws_open(port):
    sock = socket.create_connection(("127.0.0.1", port), timeout=3)
    key = base64.b64encode(os.urandom(16)).decode()
    sock.sendall((f"GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n"
                  f"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                  "Sec-WebSocket-Version: 13\r\n\r\n").encode())
    response = bytearray()
    while not response.endswith(b"\r\n\r\n"):
        part = sock.recv(1)
        assert part and len(response) < 8192, "Closed or oversized WebSocket upgrade"
        response.extend(part)
    assert response.startswith(b"HTTP/1.1 101"), "WebSocket upgrade failed"
    return sock


def ws_send(sock, message):
    payload = bytes(5) + json.dumps(message).encode()
    mask = os.urandom(4)
    header = bytes([0x82, 0x80 | len(payload)]) if len(payload) < 126 else b"\x82\xfe" + struct.pack("!H", len(payload))
    sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))


def exact(sock, count):
    data = bytearray()
    while len(data) < count:
        part = sock.recv(count - len(data))
        assert part, "Unexpected closed WebSocket"
        data.extend(part)
    return bytes(data)


def ws_message(sock):
    while True:
        opcode, length = exact(sock, 2)
        length &= 127
        if length == 126:
            length, = struct.unpack("!H", exact(sock, 2))
        elif length == 127:
            length, = struct.unpack("!Q", exact(sock, 8))
        assert length <= 65536, "Unbounded WebSocket frame"
        payload = exact(sock, length)
        if opcode & 15 == 2:
            return json.loads(payload[5:])
        assert opcode & 15 != 8, "Unexpected WebSocket close"


def smoke(port, token):
    def request(path, expected, auth=False, method="GET"):
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
        conn.request(method, path, headers={"Authorization": f"Bearer {token}"} if auth else {})
        response = conn.getresponse()
        assert response.status == expected, (path, response.status)
        response.read()
        conn.close()
    request("/healthz", 200)
    request("/api/launchers", 401)
    request("/api/launchers", 200, auth=True)
    request("/api/unknown", 403, auth=True, method="POST")
    with ws_open(port) as sock:
        ws_send(sock, {"type": "hello", "token": "deliberately-wrong", "proto": 1})
        assert ws_message(sock)["type"] == "error"
    with ws_open(port) as sock:
        ws_send(sock, {"type": "hello", "token": token, "proto": 1})
        assert ws_message(sock)["type"] == "hello-ok"
        ws_send(sock, {"type": "new-session", "name": None, "command": ["demo"], "cwd": None})
        assert ws_message(sock)["type"] == "session-event"
    print("PASS: inbound HTTP, authenticated API, denied API mutation, denied wrong WS token, authorized binary WS hello and virtual session")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--runc", default="runc")
    parser.add_argument("--profile", type=Path, default=HERE / "seccomp.json")
    args = parser.parse_args()
    assert os.geteuid() == 0, "Run as root in an isolated local Linux test environment"
    binary = args.binary.resolve()
    subprocess.run(["file", str(binary)], check=True)
    headers = subprocess.check_output(["readelf", "-l", str(binary)], text=True)
    assert "INTERP" not in headers, "Runtime executable must be static"
    with tempfile.TemporaryDirectory(prefix="gateterm-demo-oci-") as temp:
        temp = Path(temp)
        rootfs = temp / "rootfs"
        rootfs.mkdir(mode=0o755)
        (rootfs / "proc").mkdir()
        (rootfs / "dev").mkdir()
        shutil.copy2(binary, rootfs / "gateterm-demo")
        subprocess.run(["musl-gcc", "-static", "-Os", "-Wall", "-Wextra", "-Werror", str(HERE / "seccomp_probe.c"), "-o", str(rootfs / "probe")], check=True)
        identity = "gateterm-demo-" + uuid.uuid4().hex[:12]
        token = secrets.token_urlsafe(32)
        config = {
            "ociVersion": "1.0.2",
            "process": {"terminal": False, "user": {"uid": 65532, "gid": 65532},
                        "args": ["/probe"], "env": ["TOKIO_WORKER_THREADS=2", f"GATETERM_DEMO_TOKENS={token}"],
                        "cwd": "/", "noNewPrivileges": True,
                        "capabilities": {k: [] for k in ["bounding", "effective", "inheritable", "permitted", "ambient"]},
                        "rlimits": [{"type": "RLIMIT_NOFILE", "hard": 256, "soft": 256}, {"type": "RLIMIT_CORE", "hard": 0, "soft": 0}]},
            "root": {"path": str(rootfs), "readonly": True},
            "hostname": "virtual-demo",
            "mounts": [{"destination": "/proc", "type": "proc", "source": "proc", "options": ["nosuid", "noexec", "nodev", "ro"]},
                       {"destination": "/dev", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid", "noexec", "mode=755", "size=65536"]}],
            "linux": {"namespaces": [{"type": n} for n in ["pid", "mount", "uts", "ipc", "cgroup"]],
                      "devices": [{"path": "/dev/null", "type": "c", "major": 1, "minor": 3, "fileMode": 438, "uid": 0, "gid": 0}],
                      "cgroupsPath": "/" + identity,
                      "resources": {"devices": [{"allow": False, "access": "rwm"}, {"allow": True, "type": "c", "major": 1, "minor": 3, "access": "rw"}], "memory": {"limit": 134217728, "swap": 134217728}, "pids": {"limit": 64}, "cpu": {"quota": 50000, "period": 100000}},
                      "seccomp": json.loads(args.profile.read_text())}}
        config_path = temp / "config.json"
        config_path.write_text(json.dumps(config))
        command = [args.runc, "--root", str(temp / "runc")]
        subprocess.run(command + ["run", "--bundle", str(temp), identity], check=True)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        config["process"]["args"] = ["/gateterm-demo", "--host", "127.0.0.1", "--port", str(port)]
        config_path.write_text(json.dumps(config))
        log = open(temp / "service.log", "wb")
        process = subprocess.Popen(command + ["run", "--bundle", str(temp), identity], stdout=log, stderr=log)
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError((temp / "service.log").read_text())
                try:
                    conn = socket.create_connection(("127.0.0.1", port), timeout=0.1)
                    conn.close()
                    break
                except OSError:
                    time.sleep(0.05)
            smoke(port, token)
            cg = Path("/sys/fs/cgroup") / identity
            assert (cg / "memory.max").read_text().strip() == "134217728"
            assert (cg / "memory.swap.max").read_text().strip() == "0"
            assert (cg / "pids.max").read_text().strip() == "64"
            assert (cg / "cpu.max").read_text().strip() == "50000 100000"
            print("PASS: actual cgroup limits memory=128MiB, swap=0, pids=64, cpu=0.5")
        finally:
            subprocess.run(command + ["kill", identity, "KILL"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            process.wait(timeout=10)
            subprocess.run(command + ["delete", "--force", identity], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            log.close()


if __name__ == "__main__":
    main()
