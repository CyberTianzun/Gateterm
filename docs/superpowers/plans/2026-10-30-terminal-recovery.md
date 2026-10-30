# Terminal switching recovery implementation plan

> **For agentic workers:** Use test-driven-development and independent component tasks; review the complete diff before committing.

**Goal:** Fix the reproduced incomplete terminal repaint paths during switching, resync, and resizing, then commit the verified changes on the current dev branch.

**Architecture:** Keep the existing wire protocol. Restore the server terminal state needed by subsequent output and retain parser continuation across snapshot boundaries. Serialize browser writes and control operations. Only authoritative server geometry resizes native terminal buffers.

**Tech Stack:** Rust, alacritty_terminal/vte, TypeScript, xterm.js, GPUI.

**Spec:** User-authorized repairs follow the preceding investigation and its minimal reproductions.

## Constraints

- Preserve multiplexing, byte-sequence accounting, and reconnect behavior.
- No forced PTY resize or full redraw on every sidebar click.
- Add regression tests that fail before the corresponding fix.
- Commit locally; pushing is outside this request.

## Tasks

- [x] Server: port snapshot continuation reproductions into grid tests; repair sync-buffer/partial-sequence recovery, alternate screen, scroll regions, and inherited cursor attributes. Keep snapshot geometry and sequence atomic.
- [x] Web: reproduce queued writes overtaken by resize/reset; serialize writes/control operations and ensure stale session frames cannot corrupt a replacement terminal. Test callbacks, disposal, and reconnect sequence tracking.
- [x] Native: stop applying proposed viewport geometry to the parser before authoritative resize; test the production geometry decision and in-flight output behavior.
- [x] Integration: verify resize/output event ordering on the server, update the documented recovery limitations, run affected full suites and a browser check, obtain independent code review, fix important findings, and commit.

## Review focus

- Snapshot taken inside synchronized output or inside UTF-8/ANSI sequences.
- Alternate-screen exit, saved cursor, non-default origin/wrap and scrolling regions.
- Web data, resize/reset, and resync-end arriving in the same browser turn.
- Switching away while browser writes are pending; resume anchors must match applied bytes.
- Multiple clients requesting different dimensions; server output must retain geometry order.

## Verification

- Rust server and client-core unit/integration tests.
- Web complete Node test suite, TypeScript check, build, and real Chrome/xterm regression.
- Native targeted tests/build when available in the local dependency/toolchain environment.
- git diff --check, reviewed staged diff, local commit, clean working tree.

## Validation and review

- Server/client-core full Cargo suite, Web 148 tests, typecheck/build, native 31 tests, and Chrome recovery/mouse scripts pass.
- Independent review findings fixed: hidden primary history, delayed authentication facts, and ambiguous replay at a no-output resize anchor.
- The uncommon saved G1 charset difference between emulators is documented in DESIGN.md; complete historical scrollback transfer remains outside the snapshot contract.
