# Windows 10 input compatibility

**Goal:** Restore keyboard input on Windows 10 ConPTY without removing precise key encoding on supported Windows hosts.

**Evidence:** On Windows 10 build 19045.6466, ordinary VT commands execute; exact browser win32-input-mode packets for A and Enter produce no output.

**Design:** Add optional `hello-ok.win32_input_mode` (default false). The server conservatively advertises support only on Windows build 22000 or later; unknown builds use VT. Keep `os` truthful. Web, Android and desktop clients use the capability rather than inferring it from `os`. Old servers safely fall back. Older clients ignore the additive field and must be updated for this fix.

**Alternatives:** Disabling precise keys on every Windows system loses working behavior; bundling a replacement ConPTY adds a deployment dependency. An additive capability reuses the existing handshake.

- [x] Add failing browser and protocol regressions for disabled/absent capability.
- [x] Implement server detection and additive handshake field; update protocol documentation and fixtures.
- [x] Propagate capability through browser, native desktop and Android connection/session state, including reconnects.
- [x] Run browser suite/typecheck/build, Rust suites, desktop checks and Android unit tests; package a Windows server containing the fixed browser assets.
- [x] Review changes and verify generated browser encoding against the affected Windows 10 server using isolated diagnostic sessions.

**Limits:** VT cannot preserve every modified key identity on older ConPTY. Remote service replacement/restart must not interrupt existing user sessions.

**Verification notes:** Initial workspace run hit the existing preview startup timeout under parallel build load; targeted rerun and complete workspace rerun passed. Android full invocation required ANDROID_NDK_HOME; unit tests passed with `-x buildCloudFfi`, reusing existing generated FFI sources. Independent review found a selection-copy regression in a proposed handler change; restored Windows shortcut protection and verified it in the real browser. Remote diagnostic sessions were removed; no remote service restart or replacement was performed.
