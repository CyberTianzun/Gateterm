# Gateterm virtual demo (local review only)

`gateterm-demo` is a separate Rust service for clean terminal screenshots and client review. Its banner and session names identify it as **Virtual demo — no host shell / no real files**. It speaks the existing Gateterm v1 binary WebSocket protocol; Android 0.3.9 needs no changes.

Build and run from the repository root:

```powershell
cargo build -p gateterm-demo --locked
$env:GATETERM_DEMO_TOKENS = 'choose-your-own-local-demo-token'
.\target\debug\gateterm-demo.exe
```

Or supply `--token TOKEN` explicitly; repeat it for isolated workspaces. CLI tokens take precedence over `GATETERM_DEMO_TOKENS`, whose format is comma-separated tokens. There is no default token; empty, duplicate, oversized or more than 16 tokens are rejected. Tokens are never printed. The default listener is `127.0.0.1:17685`; `--host ::1` and `--port PORT` are supported. Non-loopback bind addresses are refused in this local-review build.

Connect the Android emulator to `http://10.0.2.2:17685/#token=YOUR_TOKEN`; desktop clients can use `http://127.0.0.1:17685/#token=YOUR_TOKEN`. The fragment is client configuration and never reaches the HTTP server. Native clients use `/ws` and the normal binary `hello`. Opening `/` only displays an informational demo page. `/healthz` returns `ok`.

Example virtual terminal workflow (send each line with Enter):

```text
help
ls
mkdir notes
echo 'Hello from the virtual demo' > notes/hello.txt
echo 'All data stays in memory' >> notes/hello.txt
cat notes/hello.txt
cd notes
pwd
touch empty.txt
ls
```

The virtual filesystem starts at `/workspace` with `README.txt` and `example.txt`. `help`, `pwd`, `ls [path]`, `cd [path]`, `mkdir path`, `touch path`, `cat path`, `echo [words...]` and `clear` are the complete language. `cat`, `mkdir` and `touch` take exactly one path. `echo` optionally ends with `> path` or `>> path`; failed writes preserve existing bytes. Words support single/double quotes and backslash escaping outside single quotes. There is no variable expansion, globbing, history, shell option parsing or executable dispatch. Unquoted pipes, semicolons, `&`, `$`, backticks, parentheses and `<` fail safely. Quoted metacharacters are literal text. The only accepted `new-session.command` values are null/default and exactly `["demo"]`.

Paths resolve lexically inside an in-memory `/`. Absolute paths are virtual. Traversal above `/`, NUL/control characters, backslashes, colons, names over 255 bytes and normalized paths over 1024 bytes are rejected. A missing/invalid new-session cwd falls back to `/workspace`. No path is opened, canonicalized or forwarded to the host.

Each token owns a shared VFS and up to eight sessions. Each session owns its cwd, input editor, terminal grid and output sequence. Reconnecting with the same token retains everything in memory, as required by protocol CH6; the earlier per-connection design would lose sessions on disconnect. Different tokens cannot see one another's files, sessions or events. Multiple clients may attach a session concurrently: they see the same output, and geometry uses the smallest requested width and height. Detaching the last client preserves geometry. Resize watermarks force resync for anchors at or before a grid resize, including shrink/grow cycles that end at the original dimensions. Killing a session removes its screen/editor; shared virtual files remain until operator restart. Restart resets everything.

Resource bounds:

| Resource | Limit |
| --- | --- |
| Configured tokens / global WebSockets (including unauthenticated) | 16 / 16 |
| Sessions per token / attachments per connection | 8 / 8 |
| VFS entries per token, including `/` and initial entries | 64 |
| Virtual file bytes / total content bytes per token | 8192 / 65536 |
| Input line / parser tokens per command | 4096 bytes / 64 |
| Incoming or outgoing protocol payload | 65536 bytes |
| Session replay ring / terminal scrollback | 65536 bytes / zero rows |
| Outbound queue per connection | 64 messages |
| Geometry | 20×5 minimum, 160×60 maximum |
| Emitted zero-width Unicode scalars per session | 1024; further scalars display as `?` |

The 60-row cap leaves room for full dense Unicode screen previews after base64/JSON expansion. The combining-mark budget prevents unbounded per-cell storage and also applies to the live byte stream, so repaint and clients agree. Virtual file content is unchanged by this display budget. UTF-8 may be split across input frames; ordinary Unicode text, CR/LF, backspace, Ctrl+C and Ctrl+L are supported. Arrow/history editing is outside this small demo language. Backspace redraws the current line and handles multibyte characters. Input frames whose output exceeds the bounded processing budget discard the remainder with an explicit error; no frame produces more than 64KiB output.

Slow consumers are disconnected when their bounded queue fills or a socket write takes more than five seconds. Other clients continue; the workspace remains available for replay/resync on reconnect. No output is silently skipped on an open connection. Authentication requires a binary hello within five seconds. Server Ping is every 30 seconds; read idle timeout is 90 seconds.

Every `/api` route requires the same Bearer token, including unknown paths. Safe GET routes return a single virtual launcher, disconnected Cloud status, empty agents/sessions/pending, `transcript_enabled:false`, and a static update response with no update/check timestamp. Other authenticated GET routes return 404; all authenticated API mutations return 403. All other paths return 404. There are no host features, discovery, hooks, agent history, file/image upload, arbitrary static-path serving, forwarding or real AI execution.

The production dependency tree has no Gateterm server, PTY runtime or HTTP client. The service never calls process spawning, host file APIs or outbound networking. It only reads its CLI/environment configuration, listens locally and maintains bounded memory. Test-only HTTP clients talk to ephemeral loopback listeners. No Docker, public deployment, cloud registration or TLS setup is included or authorized by this local artifact.

Validation:

```powershell
cargo test -p gateterm-demo --locked
cargo clippy -p gateterm-demo --all-targets --locked -- -D warnings
cargo build -p gateterm-demo --locked
```
