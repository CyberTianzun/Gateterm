# Linux review service

This runtime contains one static Linux amd64 executable. It serves a bounded,
explicitly virtual terminal; it cannot execute commands or read real files.
The listener stays on `127.0.0.1:17685`. Existing host-network nginx supplies
HTTPS and WebSocket ingress on `gateterm.signal.top`.

## Build

Run from the repository root, using Docker with Linux amd64 support:

```sh
docker build --platform linux/amd64 -f crates/demo-server/deploy/Dockerfile \
  -t gateterm-demo:review .
```

The builder downloads Rust dependencies, separately from the runtime network
policy. Its pinned Rust/Alpine version can be overridden using `BUILDER_IMAGE`;
record the resolved builder digest and source commit with the resulting image.
No tokens, Cloud settings, host files or shell are copied into the runtime.

Alternatively install Rust 1.93.1, the `x86_64-unknown-linux-musl` target,
`musl-gcc` and binutils on Linux, then run:

```sh
sh crates/demo-server/deploy/build-linux.sh
mkdir -p target/demo-image
cp target/demo-linux/x86_64-unknown-linux-musl/release/gateterm-demo target/demo-image/
docker build --platform linux/amd64 -f crates/demo-server/deploy/Dockerfile.artifact \
  -t gateterm-demo:review target/demo-image
```

`Dockerfile.dockerignore` permits only workspace manifests, the lockfile and
source directories into the root-context build. Git metadata, ignored build
outputs, operator credentials and application configuration are excluded.
For a remote Docker Engine, `package-artifacts.py BINARY PROBE DESTINATION`
creates two minimal tar contexts and a SHA-256 record, plus a copy of the
seccomp profile. Each context contains exactly its Dockerfile and executable;
the probe context must only be used for the isolated policy check.

Use an immutable image digest for deployment. Keep the static artifact's SHA-256,
source commit, build command and verification output in an operator record.

## Runtime policy

`compose.yaml` applies UID/GID 65532, read-only root, no volumes, dropped
capabilities, no-new-privileges, private process namespace, 128 MiB memory,
no additional swap, 0.5 CPU, 64 PIDs and 256 descriptors. Two Tokio workers
avoid dependence on host CPU count. It exposes no container ports: host
networking exists only so nginx can reach the loopback listener.

`seccomp.json` is an amd64 default-deny syscall allowlist. It permits only
IPv4/IPv6 TCP stream socket creation (protocol 0 or 6 only), including
NONBLOCK/CLOEXEC flags. SCTP/MPTCP and other protocols are denied. `connect`,
UDP socket creation (including flags), TCP Fast Open via sendto/sendmsg/sendmmsg,
raw/Unix/netlink sockets, legacy
`socketcall`, io_uring, fork/vfork and namespace creation are denied. Existing
accepted TCP sockets can send replies. `clone` permits threads without new
namespaces; `clone3` returns ENOSYS for libc fallback. The initial `execve` is
needed by OCI startup; the read-only scratch image has only this executable.
No security claim relies on hiding secrets in the image or running a shell.

Use a dedicated random token stored outside git. Set `GATETERM_DEMO_IMAGE`
and `GATETERM_DEMO_TOKENS` through the deployment manager's private environment.
Do not print `docker compose config` after adding the token: it resolves secrets.
The token is visible only to privileged deployment administrators, as with
ordinary container environment variables. It must never be a Cloud credential.

```sh
docker compose -f crates/demo-server/deploy/compose.yaml up -d
```

The Compose client reads the seccomp JSON and sends it inline to Docker; no
runtime volume is required. With the Docker Engine API, use
`HostConfig.SecurityOpt = ["no-new-privileges:true", "seccomp=" + compact_json]`.
Do not pass a client-side path to the raw Engine API. Preserve every other
HostConfig setting from this Compose file and inspect actual runtime settings.

## Verify before routing

First run the local OCI test on an isolated Linux machine with root, `runc`,
`musl-gcc`, `file`, `readelf` and Python 3. It compiles a separate probe, applies
the exact seccomp JSON and resource limits, checks negative network/process
operations, then tests authorized/unauthorized HTTP and binary WebSocket flows.
Its probe and test credentials never enter the service image.

```sh
sudo python3 crates/demo-server/deploy/verify-runc.py \
  target/demo-linux/x86_64-unknown-linux-musl/release/gateterm-demo
```

This is an OCI/runc check. Before public routing, repeat the probe under the
actual production Docker daemon and inspect the running service's user,
read-only root, mounts, caps, no-new-privileges, seccomp, network and limits.
An isolated probe container uses only the static probe, the same restrictions,
and no token/Cloud configuration. The expected result is `PASS`; syscall
denials must be EPERM, not merely connection refusal. Also confirm the actual
demo's inbound HTTP and authenticated binary WS work with this profile.

## Add HTTPS routing

`nginx.locations.conf` is a snippet for the existing TLS server. Its exact
`/ws` and `/api` plus `/api/` locations preserve the paths expected by Android,
which derives HTTP APIs from the WebSocket origin. Preserve Cloud `/v1`, auth,
privacy and all other existing locations. The demo independently checks its
Bearer token for API access and binary hello token for WebSocket access.

Before changing nginx, back up the affected configuration outside git, merge
only these locations, run `nginx -t`, and reload only after validation. On any
failure restore the backup and validate before reloading. Do not alter existing
certificates, Cloud stack, database mounts, ports, credentials or secrets.

Before entering review instructions into Play Console, test from outside the
host: HTTPS certificate, rejected wrong/no token for API and WS, authorized
terminal creation/input/reconnect, and Android connection. The service's
informational root page and local `/healthz` need no new public nginx routes.
Restart discards all virtual state. Rollback removes these nginx locations,
validates/reloads nginx, then stops only this demo service.
