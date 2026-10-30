# 纯 Rust Kitty 协议生态与终端内核增强 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在保持 100% 纯 Rust 原生技术栈（`alacritty_terminal 0.25` + `crates/client-core` + `crates/server`）的前提下，激活并统一全端 Kitty 键盘协议（`CSI ... u`）、`OSC 52` 与 Kitty `OSC 5522` 剪贴板协议、粘贴安全防护、服务端 PTY 排空/解析并发解耦、花式下划线与 `OSC 8` 超链接，以及纯 Rust Kitty 图形协议（`APC G`）。

**Architecture:** 服务端 `SessionGrid` 与客户端 `ClientTerm` 开启 `alacritty_terminal` 内置的 `kitty_keyboard: true` 与剪贴板事件监听，并在 `crates/client-core` 中新增统一的纯 Rust 键盘编码器（`keyboard.rs`）、剪贴板协议模块（`clipboard.rs`）和图形协议模块（`graphics.rs`）。Desktop Native 直接调用 `client-core`，Android 通过 `crates/cloud-ffi`（UniFFI）调用，Web 端在 `keys.ts` 与 `queries.ts` 中 1:1 对齐；服务端 `pty.rs` 拆分为独立 PTY 排空线程与批量合并 VT 解析线程。

**Tech Stack:** Rust 2024 (`alacritty_terminal 0.25`, `base64 0.22`, `flate2 1`, `png 0.18`, `uniffi`) / TypeScript (`@xterm/xterm`) / GPUI (`clients/desktop-native`) / Kotlin Jetpack Compose (`clients/android`).

**Spec:** `docs/superpowers/specs/2026-10-30-kitty-ecosystem-design.md`

## Global Constraints

- **100% 纯 Rust 原生构建**：绝不引入 Zig / C++ 等外部编译工具链依赖；仅复用 `Cargo.lock` 已有的纯 Rust crate（`alacritty_terminal = "0.25"`, `base64 = "0.22"`, `flate2 = "1"`, `png = "0.18"`）。
- **线协议向后兼容**：不修改 `PROTO_VERSION = 1`（遵守 `docs/protocol.md` 规则 `X4`）。
- **查询只由服务端应答（协议规则 `D7`）**：`CSI ? u` 与 `APC G` 应答仅由服务端 `SessionGrid` 写回 PTY；客户端必须静默吞掉查询。
- **二进制单元格步幅不变**：`crates/client-core/src/term.rs` 的 `PACKED_CELL_BYTES = 20` 保持不变，新增花式下划线与超链接标志复用 `attrs: u16` 的高比特位（Bit 7..Bit 11）。
- **剪贴板与图形安全边界**：`OSC 5522` 单次写入上限 `MAX_CLIPBOARD_WRITE_BYTES = 64 * 1024 * 1024`（64 MiB），读取需用户授权；Kitty Graphics `APC G` 仅允许内联直接传输（`t=d`），拒绝本地文件传输（`t=f` / `t=t`）并返回 `EINVAL:file transmission disabled`，单会话图片缓存上限 `MAX_GRAPHICS_CACHE_BYTES = 32 * 1024 * 1024`（32 MiB），单图最大尺寸 `4096 × 4096`。
- **代码质量门禁**：每个 Rust Task 结束时 `cargo test` 与 `cargo clippy --workspace -- -D warnings` 必须通过；Web Task 结束时 `npm test` 与 `npm run typecheck` 必须通过。

## Review Focus

1. **跨分块（Chunk Boundary）切开的转义序列**：`CSI > flags u`、`OSC 5522` 或 `APC G`（`\x1b_G...\x1b\\`）在任意字节处被切分为两次 `advance()`/`write()` 调用时，必须完整重组且绝不向终端网格泄漏半截控制字符（在 Task 1、Task 5、Task 8 中验证）。
2. **主屏与副屏（Alt-Screen）独立的 Kitty 键盘模式栈**：进入副屏（`\x1b[?1049h`）并压入 `\x1b[>1u` 后，退出副屏（`\x1b[?1049l`）必须自动恢复主屏的 `kitty_keyboard_flags == 0`，防止 TUI 退出后主 Shell 残留 `CSI u` 模式导致乱码（在 Task 1、Task 3 中验证）。
3. **括号粘贴内的嵌套 `\x1b[201~` 逃逸构造**：如 `"a\x1b[20\x1b[201~1~b"` 这种单次替换后仍能拼出 `\x1b[201~` 的恶意文本，`encode_paste` 必须直接剥离所有 `0x1b`（`ESC`）字节，彻底阻断括号粘贴逃逸（在 Task 4 中验证）。
4. **Windows `win32-input-mode` 对 Kitty 键盘模式的绝对优先权**：当服务端为 `windows` 且 `win32_input_mode == true` 时，即使终端内程序开启了 `kitty_keyboard_flags > 0`，`encode_key` 仍必须输出 `win32-input-mode`（`CSI Vk;Sc;Uc;Kd;Cs;Rc _`），确保 ConPTY 能还原原生 `KEY_EVENT_RECORD`（在 Task 2、Task 3 中验证）。
5. **Kitty Graphics `APC G` 的静默标志（`q=1` / `q=2`）精确行为**：`q=1` 抑制成功应答（`OK`）但仍返回错误应答（`EINVAL`），`q=2` 同时抑制成功与错误应答；搞混会导致 `kitten icat` 挂起或向静默程序吐出多余字节（在 Task 8 中验证）。

## File Structure

```
crates/client-core/Cargo.toml                    新增 base64 / flate2 / png 纯 Rust 依赖
crates/client-core/src/lib.rs                    导出 keyboard, clipboard, graphics 模块
crates/client-core/src/term.rs                   开启 kitty_keyboard: true，扩展 ClientTermState / attrs / paste / FFI 辅助方法
crates/client-core/src/keyboard.rs               纯 Rust 统一键盘编码器 (win32 -> Kitty CSI-u -> modifyOtherKeys -> Legacy VT)
crates/client-core/src/clipboard.rs              OSC 52 事件收集与纯 Rust Kitty OSC 5522 / DECSET 5522 剪贴板协议
crates/client-core/src/graphics.rs               纯 Rust Kitty Graphics Protocol (APC G) 解析、LRU 缓存与视口锚点计算
crates/cloud-ffi/src/lib.rs                      向 UniFFI (TermEngine / TermState) 暴露 kitty_keyboard_flags, encode_key, paste/clipboard/graphics 接口
crates/server/Cargo.toml                         新增 gateterm-client-core 依赖以复用 graphics / stream filter
crates/server/src/session/grid.rs                开启 kitty_keyboard: true，CSI ? u 应答，resync 恢复 \x1b[>{flags}u，接入 APC G 应答与花式下划线 SGR
crates/server/src/session/pty.rs                 拆分独立 PTY 排空线程 (Drain Thread) 与批量合并 VT 解析线程 (Parser Thread)
clients/desktop-native/src/terminal/input.rs     委托 gateterm_client_core::keyboard::encode_key 进行终端按键编码
clients/desktop-native/src/terminal/element.rs   渲染花式下划线 (UNDERCURL 等) 与 Kitty Graphics 视口图片图层
clients/desktop-native/src/terminal/view.rs      接入多行粘贴确认、OSC 52/5522 剪贴板事件与 OSC 8 超链接安全点击
clients/android/.../term/AlacrittyTerm.kt        同步 TermState.kittyKeyboardFlags、富属性常量与 TermEngine 新增方法
clients/android/.../term/TerminalKeyboard.kt     委托 TermEngine.encodeKey 统一编码，保留物理按键按下状态表
clients/android/.../term/TerminalView.kt         接入粘贴确认、剪贴板事件与视口图片绘制
clients/web/src/queries.ts                       吞掉 CSI ? u 键盘查询与 APC G 图形查询
clients/web/src/keys.ts                          实现与 client-core 1:1 对齐的 Kitty CSI-u 编码、模式栈与粘贴逃逸清洗
clients/web/src/main.ts                          注册 CSI >/< /= u 与 ALT_SCREEN 跟踪、多行粘贴确认及 OSC 52/5522 剪贴板处理
```

---

### Task 1: 激活 `alacritty_terminal` Kitty 键盘协议状态机与 `resync` 恢复

**Files:**
- Modify: `crates/client-core/src/term.rs:66-99,205-315`
- Modify: `crates/cloud-ffi/src/lib.rs:155-225`
- Modify: `crates/server/src/session/grid.rs:42-105,175-235`
- Modify: `clients/desktop-native/src/terminal/input.rs:469-484`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt:30-69`

**Interfaces:**
- Consumes: `alacritty_terminal::term::{Config, TermMode}`
- Produces:
  - `crates/client-core/src/term.rs`: `ClientTermState::kitty_keyboard_flags: u8` (`1`=DISAMBIGUATE_ESC_CODES, `2`=REPORT_EVENT_TYPES, `4`=REPORT_ALTERNATE_KEYS, `8`=REPORT_ALL_KEYS_AS_ESC, `16`=REPORT_ASSOCIATED_TEXT)
  - `crates/client-core/src/term.rs`: `pub fn kitty_flags_from_mode(mode: TermMode) -> u8`
  - `crates/cloud-ffi/src/lib.rs`: `TermState::kitty_keyboard_flags: u8`
  - `crates/server/src/session/grid.rs`: `SessionGrid::repaint()` appends `\x1b[>{flags}u` when `kitty_keyboard_flags != 0`

- [ ] **Step 1: 在 `crates/client-core/src/term.rs` 与 `crates/server/src/session/grid.rs` 编写失败测试**

在 `crates/client-core/src/term.rs` 的 `mod tests` 末尾添加：

```rust
    #[test]
    fn tracks_kitty_keyboard_mode_stack_and_alt_screen_isolation() {
        let mut t = ClientTerm::new(20, 5, 100);
        assert_eq!(t.state().kitty_keyboard_flags, 0);

        // 跨分块推入 CSI > 3 u (DISAMBIGUATE_ESC_CODES | REPORT_EVENT_TYPES)
        t.write(b"\x1b[>");
        let st = t.write(b"3u");
        assert_eq!(st.kitty_keyboard_flags, 3);

        // 进入 alt-screen，压入 CSI > 1 u，再退出 alt-screen 应恢复主屏的 3
        t.write(b"\x1b[?1049h\x1b[>1u");
        assert_eq!(t.state().kitty_keyboard_flags, 1);
        let st_back = t.write(b"\x1b[?1049l");
        assert_eq!(st_back.kitty_keyboard_flags, 3);

        // 弹出栈恢复为 0
        let st_pop = t.write(b"\x1b[<1u");
        assert_eq!(st_pop.kitty_keyboard_flags, 0);
    }
```

在 `crates/server/src/session/grid.rs` 的 `mod tests` 末尾添加：

```rust
    #[test]
    fn kitty_keyboard_query_and_repaint_roundtrip() {
        let mut a = SessionGrid::new(20, 5);
        a.advance(b"\x1b[?u");
        assert_eq!(String::from_utf8(a.take_pty_replies()).unwrap(), "\x1b[?0u");

        a.advance(b"\x1b[>5u\x1b[?u");
        assert_eq!(String::from_utf8(a.take_pty_replies()).unwrap(), "\x1b[?5u");

        let repaint = a.repaint();
        assert!(String::from_utf8_lossy(&repaint).contains("\x1b[>5u"));

        let mut b = SessionGrid::new(20, 5);
        b.advance(&repaint);
        b.advance(b"\x1b[?u");
        assert_eq!(String::from_utf8(b.take_pty_replies()).unwrap(), "\x1b[?5u");
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-client-core tracks_kitty_keyboard_mode_stack && cargo test -p gateterm-server kitty_keyboard_query_and_repaint`
Expected: FAIL (`no field kitty_keyboard_flags on type ClientTermState`)

- [ ] **Step 3: 开启 `kitty_keyboard: true` 并暴露 `kitty_keyboard_flags`**

1. 在 `crates/client-core/src/term.rs` 中：
   - 给 `ClientTermState` 增加 `pub kitty_keyboard_flags: u8`。
   - 实现 `pub fn kitty_flags_from_mode(mode: TermMode) -> u8`，映射：
     - `TermMode::DISAMBIGUATE_ESC_CODES` -> `1`
     - `TermMode::REPORT_EVENT_TYPES` -> `2`
     - `TermMode::REPORT_ALTERNATE_KEYS` -> `4`
     - `TermMode::REPORT_ALL_KEYS_AS_ESC` -> `8`
     - `TermMode::REPORT_ASSOCIATED_TEXT` -> `16`
   - 在 `ClientTerm::new`、`ClientTerm::reset`、`ClientTerm::set_max_scrollback` 中构造 `Config` 时设置 `kitty_keyboard: true`。
   - 在 `ClientTerm::state()` 中设置 `kitty_keyboard_flags: kitty_flags_from_mode(mode)`。
2. 在 `crates/cloud-ffi/src/lib.rs`（及 `crates/cloud-ffi/tests/ffi.rs`）、`clients/desktop-native/src/terminal/input.rs` 的 `dummy_state()`、`clients/android/.../AlacrittyTerm.kt` 中同步 `kitty_keyboard_flags: u8` 字段。
3. 在 `crates/server/src/session/grid.rs` 中：
   - `SessionGrid::new` 使用 `Config { kitty_keyboard: true, ..Config::default() }`。
   - `SessionGrid::repaint()` 提取 `kitty_flags`（同上 5 个位标志），若 `kitty_flags != 0`，在 `out` 末尾追加 `format!("\x1b[>{kitty_flags}u")`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p gateterm-client-core -p gateterm-cloud-ffi -p gateterm-server`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/src/term.rs crates/cloud-ffi/src/lib.rs crates/server/src/session/grid.rs clients/desktop-native/src/terminal/input.rs clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt
git commit -m "feat(term): enable alacritty kitty keyboard protocol and resync restoration"
```

---

### Task 2: 在 `crates/client-core` 实现纯 Rust 统一键盘编码器并接入 Desktop Native 与 Android

**Files:**
- Create: `crates/client-core/src/keyboard.rs`
- Modify: `crates/client-core/src/lib.rs:1-5`
- Modify: `crates/client-core/src/term.rs:455-475`
- Modify: `crates/cloud-ffi/src/lib.rs:170-285`
- Modify: `crates/cloud-ffi/tests/ffi.rs`
- Modify: `clients/desktop-native/src/terminal/input.rs`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/TerminalKeyboard.kt`

**Interfaces:**
- Consumes: `ClientTermState` (from Task 1)
- Produces:
  - `crates/client-core/src/keyboard.rs`:
    - `pub enum KeyEventKind { Press, Repeat, Release }`
    - `pub struct KeyInput { pub key: String, pub codepoint: Option<u32>, pub shifted_codepoint: Option<u32>, pub base_layout_codepoint: Option<u32>, pub shift: bool, pub alt: bool, pub ctrl: bool, pub super_key: bool, pub kind: KeyEventKind }`
    - `pub fn encode_key(input: &KeyInput, state: &ClientTermState, server_os: &str, win32_input_mode: bool) -> Vec<u8>`
  - `crates/client-core/src/term.rs`: `ClientTerm::encode_key(&self, input: &KeyInput, server_os: &str, win32_input_mode: bool) -> Vec<u8>`
  - `crates/cloud-ffi/src/lib.rs`: `TermEngine::encode_key(&self, key: String, codepoint: u32, shift: bool, alt: bool, ctrl: bool, is_release: bool, server_os: String, win32_input_mode: bool) -> Vec<u8>`

- [ ] **Step 1: 在 `crates/client-core/src/keyboard.rs` 编写失败测试**

在 `crates/client-core/src/lib.rs` 声明 `pub mod keyboard;`，并在 `crates/client-core/src/keyboard.rs` 编写单元测试覆盖 4 层优先级与 Review Focus #4：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::ClientTermState;

    fn base_state() -> ClientTermState {
        ClientTermState {
            cols: 80, rows: 24, cursor_x: 0, cursor_y: 0, cursor_visible: true,
            application_cursor: false, bracketed_paste: false, modify_other_keys: 0,
            kitty_keyboard_flags: 0, scroll_offset: 0, max_scroll_offset: 0, mouse_tracking: false,
        }
    }

    #[test]
    fn kitty_disambiguate_and_event_types_encode_csi_u() {
        let mut st = base_state();
        st.kitty_keyboard_flags = 1; // DISAMBIGUATE_ESC_CODES
        // Shift+Enter -> CSI 13;2u
        let shift_enter = KeyInput::simple("enter", Some(13), true, false, false, KeyEventKind::Press);
        assert_eq!(encode_key(&shift_enter, &st, "linux", false), b"\x1b[13;2u");
        // Plain Enter in flag=1 stays CR
        let plain_enter = KeyInput::simple("enter", Some(13), false, false, false, KeyEventKind::Press);
        assert_eq!(encode_key(&plain_enter, &st, "linux", false), b"\r");
        // Escape in flag=1 -> CSI 27u
        let esc = KeyInput::simple("escape", Some(27), false, false, false, KeyEventKind::Press);
        assert_eq!(encode_key(&esc, &st, "linux", false), b"\x1b[27u");
        // Ctrl+J in flag=1 -> CSI 106;5u
        let ctrl_j = KeyInput::simple("j", Some(b'j' as u32), false, false, true, KeyEventKind::Press);
        assert_eq!(encode_key(&ctrl_j, &st, "linux", false), b"\x1b[106;5u");
        // Release ignored when REPORT_EVENT_TYPES (2) is off
        let rel = KeyInput::simple("j", Some(b'j' as u32), false, false, true, KeyEventKind::Release);
        assert!(encode_key(&rel, &st, "linux", false).is_empty());

        // Enable REPORT_EVENT_TYPES (1 | 2 = 3)
        st.kitty_keyboard_flags = 3;
        assert_eq!(encode_key(&rel, &st, "linux", false), b"\x1b[106;5:3u");
        let rep = KeyInput::simple("up", None, false, false, false, KeyEventKind::Repeat);
        assert_eq!(encode_key(&rep, &st, "linux", false), b"\x1b[1;1:2A");
    }

    #[test]
    fn windows_win32_input_mode_overrides_kitty_keyboard_flags() {
        let mut st = base_state();
        st.kitty_keyboard_flags = 31;
        let shift_enter = KeyInput::simple("enter", Some(13), true, false, false, KeyEventKind::Press);
        assert_eq!(
            encode_key(&shift_enter, &st, "windows", true),
            b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_"
        );
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-client-core keyboard::tests`
Expected: FAIL (unimplemented module/functions)

- [ ] **Step 3: 实现 `crates/client-core/src/keyboard.rs` 并接入 `ClientTerm`、`cloud-ffi`、Desktop Native 与 Android**

1. 在 `crates/client-core/src/keyboard.rs` 实现完整的 `encode_key`：
   - 迁移 `clients/desktop-native/src/terminal/input.rs` 中的 `WinKeyInfo`、`resolve_win32_key`、导航键、功能键、`modifyOtherKeys` 与控制字符逻辑。
   - 新增 Kitty `CSI ... u` 编码分支（在 `!(server_os.eq_ignore_ascii_case("windows") && win32_input_mode)` 且 `state.kitty_keyboard_flags > 0` 时生效）：
     - 修饰位计算：`mods = 1 + (shift as u32) + ((alt as u32) << 1) + ((ctrl as u32) << 2) + ((super_key as u32) << 3)`。
     - 当 `kind == KeyEventKind::Release` 且 `(flags & 2) == 0` 时返回 `Vec::new()`。
     - 功能键码映射：`Escape=27`, `Enter=13`, `Tab=9`, `Backspace=127`, `Insert=2~`, `Delete=3~`, `PageUp=5~`, `PageDown=6~`, `Up=A`, `Down=B`, `Right=C`, `Left=D`, `Home=H`, `End=F`, `F1=P`, `F2=Q`, `F3=R`/`13~`, `F4=S`, `F5=15~`..`F12=24~`。
     - 当 `REPORT_ALL_KEYS_AS_ESC (8)` 开启，或按键为 `Escape`，或按键带有 `ctrl`/`alt`/`super_key`，或 `Enter`/`Tab`/`Backspace` 带有 `shift` 时，输出 `CSI <codepoint>[:shifted[:base]] [; <mods>[:<event_type>]] [; <associated_text>] u`。
2. 在 `crates/client-core/src/term.rs` 添加 `ClientTerm::encode_key`，并在 `crates/cloud-ffi/src/lib.rs` 的 `TermEngine` 上导出 `encode_key`。
3. 在 `clients/desktop-native/src/terminal/input.rs` 中让 `encode_keystroke` 直接构造 `KeyInput` 并调用 `gateterm_client_core::keyboard::encode_key`。
4. 在 `clients/android/.../TerminalKeyboard.kt` 中支持传入 `kittyKeyboardFlags: Int = 0`（或通过 `AlacrittyTerm` 调用 `TermEngine.encodeKey`）完成 Kitty `CSI u` 编码。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --workspace && cargo test --manifest-path clients/desktop-native/Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/ crates/cloud-ffi/ clients/desktop-native/src/terminal/input.rs clients/android/
git commit -m "feat(keyboard): unify pure-Rust Kitty CSI-u and win32 key encoder in client-core"
```

---

### Task 3: Web 端 (`clients/web`) 同步 Kitty 键盘协议查询拦截、模式栈与 `CSI u` 编码

**Files:**
- Modify: `clients/web/src/queries.ts:22-35`
- Modify: `clients/web/src/keys.ts:110-211`
- Modify: `clients/web/src/main.ts`
- Modify: `clients/web/test/queries.test.mjs`
- Modify: `clients/web/test/keys.test.mjs`

**Interfaces:**
- Consumes: Server `SessionGrid` `CSI ? u` reply and `resync` `\x1b[>{flags}u` (from Task 1)
- Produces:
  - `clients/web/src/keys.ts`: `KeyboardAdapter.pushKittyFlags(flags: number)`, `KeyboardAdapter.popKittyFlags(count: number)`, `KeyboardAdapter.setKittyFlags(flags: number, mode?: number)`, `KeyboardAdapter.setAltScreen(alt: boolean)`
  - `clients/web/src/queries.ts`: swallows `CSI ? u` (`{ prefix: "?", final: "u" }`) and `APC G`

- [ ] **Step 1: 在 `clients/web/test/queries.test.mjs` 与 `clients/web/test/keys.test.mjs` 编写失败测试**

在 `clients/web/test/queries.test.mjs` 增加断言验证 `\x1b[?u` 被吞掉（不触发 `onData`）；在 `clients/web/test/keys.test.mjs` 增加测试：

```javascript
test("kitty keyboard protocol encodes CSI-u, respects alt-screen stack, and yields to win32-input-mode", () => {
  const adapter = new KeyboardAdapter();
  adapter.pushKittyFlags(3); // DISAMBIGUATE (1) | REPORT_EVENT_TYPES (2)
  assert.equal(adapter.encode(key({ key: "Enter", code: "Enter", shiftKey: true }), "linux"), "\x1b[13;2u");
  assert.equal(adapter.encode(key({ key: "Enter", code: "Enter" }), "linux"), "\r");
  assert.equal(adapter.encode(key({ key: "Escape", code: "Escape" }), "linux"), "\x1b[27u");
  assert.equal(adapter.encode(key({ key: "j", code: "KeyJ", ctrlKey: true }), "linux"), "\x1b[106;5u");
  assert.equal(adapter.encode(key({ type: "keyup", key: "j", code: "KeyJ", ctrlKey: true }), "linux"), "\x1b[106;5:3u");

  // Windows win32-input-mode still wins
  assert.equal(
    adapter.encode(key({ key: "Enter", code: "Enter", shiftKey: true }), "windows", false, true),
    "\x1b[13;28;13;1;16;1_"
  );

  // Alt-screen stack isolation
  adapter.setAltScreen(true);
  adapter.pushKittyFlags(1);
  adapter.setAltScreen(false);
  assert.equal(adapter.encode(key({ type: "keyup", key: "j", code: "KeyJ", ctrlKey: true }), "linux"), "\x1b[106;5:3u");
  adapter.popKittyFlags(1);
  assert.equal(adapter.encode(key({ key: "Enter", code: "Enter", shiftKey: true }), "linux"), null);
});
```

- [ ] **Step 2: 运行 Web 测试确认失败**

Run: `npm --prefix clients/web test`
Expected: FAIL (`adapter.pushKittyFlags is not a function`)

- [ ] **Step 3: 在 `queries.ts`、`keys.ts` 与 `main.ts` 实现 Kitty 键盘协议支持**

1. 在 `clients/web/src/queries.ts` 的 `swallowTerminalQueries` 中添加：
   `parser.registerCsiHandler({ prefix: "?", final: "u" }, swallow);`
2. 在 `clients/web/src/keys.ts` 的 `KeyboardAdapter` 中：
   - 维护主屏栈 `mainKittyStack: number[] = []`、副屏栈 `altKittyStack: number[] = []` 和 `altScreen = false`。
   - 实现 `pushKittyFlags`、`popKittyFlags`、`setKittyFlags(flags, mode = 1)`、`setAltScreen(alt)` 和 `resetKittyFlags()`（在 `resync` / `RIS` 时调用）。
   - 在 `encode()` 中，当 `!(serverOs === "windows" && win32InputMode)` 且当前栈顶 `kittyFlags > 0` 时，按与 `client-core::keyboard` 相同的规则编码 `CSI ... u`（并在 `keydown` 截获时将该 `ev.code` 记入 `this.modified` 以吞掉后续 `keypress`）。
3. 在 `clients/web/src/main.ts` 创建 `Terminal` 时注册 `CSI > u`、`CSI < u`、`CSI = u` 以及 `DECSET/DECRST 47/1047/1049` 处理器更新 `keyboard`。

- [ ] **Step 4: 运行 Web 测试与类型检查确认通过**

Run: `npm --prefix clients/web test && npm --prefix clients/web run typecheck`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add clients/web/src/queries.ts clients/web/src/keys.ts clients/web/src/main.ts clients/web/test/queries.test.mjs clients/web/test/keys.test.mjs
git commit -m "feat(web): support Kitty keyboard protocol stack and CSI-u encoding"
```

---

### Task 4: 粘贴安全加固（`\x1b[201~` 逃逸剥离与非 Bracketed Paste 多行粘贴确认）

**Files:**
- Modify: `crates/client-core/src/term.rs:459-467`
- Modify: `crates/cloud-ffi/src/lib.rs:265-275`
- Modify: `clients/web/src/keys.ts`
- Modify: `clients/web/src/main.ts`
- Modify: `clients/desktop-native/src/terminal/view.rs`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt`

**Interfaces:**
- Consumes: `ClientTerm`
- Produces:
  - `crates/client-core/src/term.rs`:
    - `ClientTerm::paste_requires_confirmation(&self, text: &str) -> bool`
    - `ClientTerm::encode_paste(&self, text: &str) -> Vec<u8>` (strips all `0x1b` / `ESC` bytes from `text` before wrapping or emitting)
  - `crates/cloud-ffi/src/lib.rs`: `TermEngine::paste_requires_confirmation(&self, text: String) -> bool`
  - `clients/web/src/keys.ts`: `export function sanitizePasteText(text: string): string`, `export function pasteRequiresConfirmation(text: string, bracketedPaste: boolean): boolean`

- [ ] **Step 1: 在 `crates/client-core/src/term.rs` 与 `clients/web/test/keys.test.mjs` 编写失败测试**

在 `crates/client-core/src/term.rs` 的 `mod tests` 添加（含 Review Focus #3 嵌套逃逸测试）：

```rust
    #[test]
    fn encode_paste_strips_nested_esc_breakout_and_flags_unbracketed_multiline() {
        let mut t = ClientTerm::new(20, 5, 100);
        // 未开启 bracketed paste 时，包含 \n 或 \r 需确认
        assert!(t.paste_requires_confirmation("curl evil | sh\n"));
        assert!(t.paste_requires_confirmation("curl evil | sh\r"));
        assert!(!t.paste_requires_confirmation("plain single line"));

        // 开启 bracketed paste 后，多行无需确认，但嵌套 \x1b[20\x1b[201~1~ 必须被彻底剥离 ESC
        t.write(b"\x1b[?2004h");
        assert!(!t.paste_requires_confirmation("line1\nline2"));
        let malicious = "safe\x1b[20\x1b[201~1~; rm -rf /\n";
        let encoded = t.encode_paste(malicious);
        let s = String::from_utf8(encoded).unwrap();
        assert!(s.starts_with("\x1b[200~"));
        assert!(s.ends_with("\x1b[201~"));
        let inner = &s["\x1b[200~".len()..s.len() - "\x1b[201~".len()];
        assert!(!inner.contains('\x1b'), "inner payload still contains ESC: {inner:?}");
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-client-core encode_paste_strips_nested_esc_breakout`
Expected: FAIL (`no method named paste_requires_confirmation`)

- [ ] **Step 3: 实现粘贴清洗与多行确认检测**

1. 在 `crates/client-core/src/term.rs` 中：
   - 实现 `pub fn paste_requires_confirmation(&self, text: &str) -> bool`：`!self.term.mode().contains(TermMode::BRACKETED_PASTE) && (text.contains('\n') || text.contains('\r'))`。
   - 修改 `encode_paste(&self, text: &str)`：先过滤掉 `text` 中的所有 `'\x1b'`（以及 `'\x9b'` 8-bit CSI），再将 `"\r\n"` 与 `"\n"` 归一为 `"\r"`，最后根据 `TermMode::BRACKETED_PASTE` 包裹 `\x1b[200~...\x1b[201~`。
2. 在 `crates/cloud-ffi/src/lib.rs` 和 `AlacrittyTerm.kt` 中暴露 `paste_requires_confirmation`。
3. 在 `clients/web/src/keys.ts` 中导出 `sanitizePasteText` 与 `pasteRequiresConfirmation`，并在 `clients/desktop-native/src/terminal/view.rs` 和 `clients/web/src/main.ts` 的粘贴入口接入。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p gateterm-client-core -p gateterm-cloud-ffi && npm --prefix clients/web test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/src/term.rs crates/cloud-ffi/src/lib.rs clients/desktop-native/src/terminal/view.rs clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt clients/web/src/keys.ts clients/web/src/main.ts clients/web/test/keys.test.mjs
git commit -m "fix(security): strip bracketed-paste ESC breakouts and guard unbracketed CR/LF pastes"
```

---

### Task 5: 打通 `OSC 52` 与纯 Rust Kitty `OSC 5522` / `DECSET 5522` 剪贴板协议

**Files:**
- Create: `crates/client-core/src/clipboard.rs`
- Modify: `crates/client-core/Cargo.toml`
- Modify: `crates/client-core/src/lib.rs`
- Modify: `crates/client-core/src/term.rs`
- Modify: `crates/cloud-ffi/src/lib.rs`
- Modify: `crates/server/src/session/grid.rs`

**Interfaces:**
- Consumes: `base64 = "0.22"`, `alacritty_terminal::event::Event::ClipboardStore`
- Produces:
  - `crates/client-core/src/clipboard.rs`:
    - `pub const MAX_CLIPBOARD_WRITE_BYTES: usize = 64 * 1024 * 1024;`
    - `pub enum ClipboardEvent { Write { mime: String, data: Vec<u8> }, ReadRequest { mimes: Vec<String> } }`
    - `pub fn encode_osc5522_read_reply(mime: &str, data: &[u8]) -> Vec<u8>`
    - `pub fn encode_osc5522_paste_offer(available_mimes: &[&str]) -> Vec<u8>`
  - `crates/client-core/src/term.rs`:
    - `ClientTermState::kitty_clipboard_paste_mode: bool` (`DECSET 5522`)
    - `ClientTerm::drain_clipboard_events(&mut self) -> Vec<ClipboardEvent>`

- [ ] **Step 1: 在 `crates/client-core/src/clipboard.rs` 编写失败测试**

在 `crates/client-core/Cargo.toml` 添加 `base64 = "0.22"`，在 `crates/client-core/src/clipboard.rs` 编写单元测试（包含跨 chunk 切分测试 Review Focus #1）：

```rust
#[cfg(test)]
mod tests {
    use crate::clipboard::{ClipboardEvent, encode_osc5522_paste_offer, encode_osc5522_read_reply};
    use crate::term::ClientTerm;

    #[test]
    fn captures_osc52_and_chunked_osc5522_writes_and_read_requests() {
        let mut t = ClientTerm::new(20, 5, 100);
        // 1. OSC 52 write ("hello" -> aGVsbG8=)
        t.write(b"\x1b]52;c;aGVsbG8=\x07");
        let evs = t.drain_clipboard_events();
        assert_eq!(evs, vec![ClipboardEvent::Write { mime: "text/plain".into(), data: b"hello".to_vec() }]);

        // 2. 跨 chunk 的 OSC 5522 多格式写入 (image/png)与读取请求
        t.write(b"\x1b]5522;type=write:mime=aW1hZ2UvcG5n;");
        t.write(b"data=iVBORw==\x1b\\");
        let evs2 = t.drain_clipboard_events();
        assert_eq!(evs2, vec![ClipboardEvent::Write { mime: "image/png".into(), data: vec![0x89, 0x50, 0x4e, 0x47] }]);

        // 3. DECSET 5522 模式开关
        t.write(b"\x1b[?5522h");
        assert!(t.state().kitty_clipboard_paste_mode);
        t.write(b"\x1b[?5522l");
        assert!(!t.state().kitty_clipboard_paste_mode);

        // 4. Read request
        t.write(b"\x1b]5522;type=read;aW1hZ2UvcG5n\x1b\\");
        assert_eq!(
            t.drain_clipboard_events(),
            vec![ClipboardEvent::ReadRequest { mimes: vec!["image/png".into()] }]
        );

        let offer = encode_osc5522_paste_offer(&["text/plain", "image/png"]);
        assert!(offer.starts_with(b"\x1b]5522;"));
        let reply = encode_osc5522_read_reply("text/plain", b"ok");
        assert!(reply.starts_with(b"\x1b]5522;"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-client-core captures_osc52_and_chunked_osc5522`
Expected: FAIL

- [ ] **Step 3: 实现 `crates/client-core/src/clipboard.rs` 并接入 `ClientTerm` 与 `SessionGrid`**

1. 将 `ClientTerm` 的 `Term<VoidListener>` 替换为自定义的 `ClientEventListener`，捕获 `Event::ClipboardStore(_, text)` 并推入 `ClipboardEvent::Write { mime: "text/plain".into(), data: text.into_bytes() }`。
2. 在 `crates/client-core/src/clipboard.rs` 实现 `Osc5522Parser`：
   - 拦截 `\x1b]5522;...(\x07|\x1b\\)` 以及 `\x1b[?5522h` / `\x1b[?5522l`。
   - 累计写入大小超过 `MAX_CLIPBOARD_WRITE_BYTES`（64 MiB）时立即丢弃当前写入并清空缓冲，不产生 `ClipboardEvent::Write`。
   - 实现 `encode_osc5522_read_reply` 与 `encode_osc5522_paste_offer`。
3. 在 `SessionGrid` 中同样记录 `kitty_clipboard_paste_mode` 并在 `repaint()` 时若为 `true` 输出 `\x1b[?5522h`。
4. 在 `crates/cloud-ffi/src/lib.rs` 暴露 `FfiClipboardEvent` 与 `drain_clipboard_events`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p gateterm-client-core -p gateterm-cloud-ffi -p gateterm-server`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/ crates/cloud-ffi/ crates/server/src/session/grid.rs clients/desktop-native/ clients/android/
git commit -m "feat(clipboard): wire OSC 52 and implement pure-Rust Kitty OSC 5522 clipboard protocol"
```

---

### Task 6: 解耦服务端 `pty.rs` 的 PTY 排空线程与批量 VT 解析线程

**Files:**
- Modify: `crates/server/src/session/pty.rs:270-330`
- Test: `crates/server/tests/session_pty.rs`

**Interfaces:**
- Consumes: `SessionGrid::advance`, `SeqBuffer::append`
- Produces:
  - `crates/server/src/session/pty.rs`: Dedicated PTY drain thread sending raw chunks over `std::sync::mpsc::sync_channel::<Vec<u8>>(256)` to a coalescing parser thread (up to `COALESCE_PARSE_MAX = 64 * 1024` bytes per `shared.lock()`).

- [ ] **Step 1: 在 `crates/server/src/session/pty.rs` 编写并发合并单元测试**

在 `crates/server/src/session/pty.rs` 的 `mod tests` 中添加测试，验证多个小切片（模拟 macOS 1 KiB 截断）在解析端能够无损合并且保持字节序与查询应答顺序：

```rust
    #[test]
    fn coalesces_queued_pty_chunks_up_to_cap_without_dropping_tail() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);
        for _ in 0..10 {
            tx.send(vec![b'a'; 1024]).unwrap();
        }
        drop(tx);
        let first = rx.recv().unwrap();
        let batch = coalesce_pty_chunks(first, &rx, 4096);
        assert_eq!(batch.len(), 4096);
        let second = rx.recv().unwrap();
        let batch2 = coalesce_pty_chunks(second, &rx, 4096);
        assert_eq!(batch2.len(), 4096);
        let third = rx.recv().unwrap();
        let batch3 = coalesce_pty_chunks(third, &rx, 4096);
        assert_eq!(batch3.len(), 2048);
        assert!(rx.recv().is_err());
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-server coalesces_queued_pty_chunks`
Expected: FAIL (`cannot find function coalesce_pty_chunks`)

- [ ] **Step 3: 实现 `coalesce_pty_chunks` 与双线程流水线**

在 `crates/server/src/session/pty.rs` 中：
1. 定义 `const COALESCE_PARSE_MAX: usize = 64 * 1024;` 和辅助函数 `coalesce_pty_chunks`（在不超出 `cap` 时通过 `rx.try_recv()` 贪婪合并，若 `try_recv` 取出的块会使总长超限则可采用 `Peekable` 或在单块 `<= 8192` 且 `batch.len() + 8192 <= cap` 时继续调用 `try_recv()`，既无需回退也不超限）。
2. 将 `Session::spawn` 中的单线程循环拆为：
   - **PTY Drain Thread**：只执行 `reader.read(&mut buf)` 并 `chunk_tx.send(buf[..n].to_vec())`，不碰 `s.shared` 锁。
   - **VT Parser Thread**：从 `chunk_rx` 接收并通过 `coalesce_pty_chunks` 合并后，加一次 `s.shared.lock()` 执行 `grid.advance(&batch)`、`buffer.append(&batch)` 和 `take_pty_replies()`，随后在锁外写回 `replies`、广播 `SessionOutput::Data`，并在 `chunk_rx` 关闭后清理子进程与广播 `SessionOutput::Exited`。

- [ ] **Step 4: 运行服务端全部测试确认通过**

Run: `cargo test -p gateterm-server`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/session/pty.rs
git commit -m "perf(server): decouple PTY drain thread from batched VT parsing"
```

---

### Task 7: 透传花式下划线 (`UNDERCURL` 等) 与安全校验的 `OSC 8` 超链接

**Files:**
- Modify: `crates/client-core/src/term.rs:33-42,619-650`
- Modify: `crates/cloud-ffi/src/lib.rs`
- Modify: `crates/server/src/session/grid.rs:150-168,261-283`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt:193-202`

**Interfaces:**
- Consumes: `alacritty_terminal::term::cell::{Cell, Flags}`
- Produces:
  - `crates/client-core/src/term.rs`:
    - `attr::DOUBLE_UNDERLINE = 128`, `attr::UNDERCURL = 256`, `attr::DOTTED_UNDERLINE = 512`, `attr::DASHED_UNDERLINE = 1024`, `attr::HYPERLINK = 2048`
    - `ClientTerm::hyperlink_at(&self, col: usize, row: usize) -> Option<String>` (returns `Some(uri)` only for safe `http://`, `https://`, `mailto:` URLs without ASCII control characters)
  - `crates/cloud-ffi/src/lib.rs`: `TermEngine::hyperlink_at(&self, col: u32, row: u32) -> Option<String>`
  - `crates/server/src/session/grid.rs`: `sgr_string` emits `4:2`, `4:3`, `4:4`, `4:5` for underline variants

- [ ] **Step 1: 在 `crates/client-core/src/term.rs` 与 `crates/server/src/session/grid.rs` 编写失败测试**

在 `crates/client-core/src/term.rs` 的 `mod tests` 添加：

```rust
    #[test]
    fn preserves_fancy_underline_flags_and_validates_osc8_hyperlinks() {
        let mut t = ClientTerm::new(40, 5, 100);
        t.write(b"\x1b[4:3mU\x1b[4:2mD\x1b[0m");
        assert_ne!(t.cell(0, 0).attrs & attr::UNDERCURL, 0);
        assert_ne!(t.cell(0, 0).attrs & attr::UNDERLINE, 0);
        assert_ne!(t.cell(1, 0).attrs & attr::DOUBLE_UNDERLINE, 0);

        // 合法 https 链接允许返回，恶意 file:// 或 javascript: 或含控制字符链接被拦截
        t.write(b"\x1b]8;;https://example.com/docs\x1b\\A\x1b]8;;\x1b\\");
        t.write(b"\x1b]8;;file:///etc/passwd\x1b\\B\x1b]8;;\x1b\\");
        assert_eq!(t.hyperlink_at(2, 0).as_deref(), Some("https://example.com/docs"));
        assert_ne!(t.cell(2, 0).attrs & attr::HYPERLINK, 0);
        assert_eq!(t.hyperlink_at(3, 0), None);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-client-core preserves_fancy_underline_flags`
Expected: FAIL (`not found in attr`)

- [ ] **Step 3: 实现花式下划线标志位、SGR 重绘与 `hyperlink_at` 安全过滤**

1. 在 `crates/client-core/src/term.rs` 的 `pub mod attr` 添加 `DOUBLE_UNDERLINE = 128`, `UNDERCURL = 256`, `DOTTED_UNDERLINE = 512`, `DASHED_UNDERLINE = 1024`, `HYPERLINK = 2048`，并在 `map_flags` / `convert_cell` 中置位（同时保留 `attr::UNDERLINE = 8` 以兼容基础渲染器）。
2. 实现 `ClientTerm::hyperlink_at(&self, col: usize, row: usize) -> Option<String>`：读取当前视口行 `Line(row as i32 - self.viewport_offset() as i32)` 上的 `cell.hyperlink()`，校验 URI 以 `https://`、`http://` 或 `mailto:` 开头且不含 `0x00..=0x1f` / `0x7f` 控制字符后返回。
3. 在 `crates/server/src/session/grid.rs` 的 `sgr_string` 中按具体标志输出 `"4:2"`, `"4:3"`, `"4:4"`, `"4:5"` 或 `"4"`。
4. 在 `crates/cloud-ffi/src/lib.rs` 和 `AlacrittyTerm.kt` 中同步导出 `hyperlink_at` 与 `Attr` 常量。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p gateterm-client-core -p gateterm-cloud-ffi -p gateterm-server`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/src/term.rs crates/cloud-ffi/src/lib.rs crates/server/src/session/grid.rs clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt
git commit -m "feat(term): expose fancy underline styles and validated OSC 8 hyperlinks"
```

---

### Task 8: 实现纯 Rust Kitty 图形协议 (`APC G` / `\x1b_G...`) 核心模块

**Files:**
- Create: `crates/client-core/src/graphics.rs`
- Modify: `crates/client-core/Cargo.toml`
- Modify: `crates/client-core/src/lib.rs`
- Modify: `crates/client-core/src/term.rs`
- Modify: `crates/cloud-ffi/src/lib.rs`
- Modify: `crates/server/Cargo.toml`
- Modify: `crates/server/src/session/grid.rs`

**Interfaces:**
- Consumes: `base64 = "0.22"`, `flate2 = "1"`, `png = "0.18"`
- Produces:
  - `crates/client-core/src/graphics.rs`:
    - `pub const MAX_GRAPHICS_CACHE_BYTES: usize = 32 * 1024 * 1024;`
    - `pub const MAX_IMAGE_DIMENSION: u32 = 4096;`
    - `pub struct ViewportImage { pub image_id: u32, pub placement_id: u32, pub col: u32, pub row: i32, pub cols: u32, pub rows: u32, pub z_index: i32, pub width_px: u32, pub height_px: u32, pub rgba: Arc<Vec<u8>> }`
    - `pub struct KittyGraphicsState`
  - `crates/client-core/src/term.rs`: `ClientTerm::visible_images(&self) -> Vec<ViewportImage>`
  - `crates/cloud-ffi/src/lib.rs`: `TermEngine::visible_images(&self) -> Vec<FfiViewportImage>`

- [ ] **Step 1: 在 `crates/client-core/src/graphics.rs` 编写失败测试**

在 `crates/client-core/Cargo.toml` 添加 `flate2 = "1"` 与 `png = "0.18"`，在 `crates/client-core/src/graphics.rs` 编写单元测试（覆盖内联 RGBA/PNG 传输、跨分块重组 Review Focus #1、`t=f` 拒绝、`q=1`/`q=2` 静默标志 Review Focus #5、无效数据返回 `EINVAL`）：

```rust
#[cfg(test)]
mod tests {
    use crate::term::ClientTerm;

    #[test]
    fn parses_chunked_inline_rgba_image_and_places_in_viewport() {
        let mut t = ClientTerm::new(20, 5, 100);
        // 1x1 red RGBA pixel ([255, 0, 0, 255] -> base64 "/wAA/w==")
        // a=T (transmit & display), f=32 (RGBA), s=1, v=1, i=7, c=2, r=1, m=1 then m=0
        t.write(b"\x1b_Ga=T,f=32,s=1,v=1,i=7,c=2,r=1,m=1;/wA");
        t.write(b"A/w==\x1b\\");
        let imgs = t.visible_images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].image_id, 7);
        assert_eq!(imgs[0].cols, 2);
        assert_eq!(imgs[0].rows, 1);
        assert_eq!(&*imgs[0].rgba, &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_file_transmission_and_honors_quiet_flags() {
        let mut g = crate::graphics::KittyGraphicsState::new();
        // t=f (local file) must be rejected with EINVAL
        let reply = g.handle_apc(b"Ga=T,t=f,i=3;L2V0Yy9wYXNzd2Q=", 0, 0, 80, 24);
        assert_eq!(
            String::from_utf8(reply.unwrap()).unwrap(),
            "\x1b_Gi=3;EINVAL:file transmission disabled\x1b\\"
        );

        // q=1 suppresses OK, but still reports EINVAL
        let ok_q1 = g.handle_apc(b"Ga=T,f=32,s=1,v=1,i=4,q=1;/wAA/w==", 0, 0, 80, 24);
        assert!(ok_q1.is_none());
        let err_q1 = g.handle_apc(b"Ga=T,t=f,i=5,q=1;L2V0Yy9wYXNzd2Q=", 0, 0, 80, 24);
        assert!(err_q1.is_some());

        // q=2 suppresses both OK and EINVAL
        let err_q2 = g.handle_apc(b"Ga=T,t=f,i=6,q=2;L2V0Yy9wYXNzd2Q=", 0, 0, 80, 24);
        assert!(err_q2.is_none());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p gateterm-client-core graphics::tests`
Expected: FAIL

- [ ] **Step 3: 实现 `crates/client-core/src/graphics.rs` 并接入 `ClientTerm`、`cloud-ffi` 与 `SessionGrid`**

1. 在 `crates/client-core/src/graphics.rs` 实现 `KittyGraphicsState`：
   - 解析 `APC G` 控制键值（`a=t|T|p|d|q`, `t=d|f|t|s`, `f=24|32|100`, `o=z`, `s`, `v`, `i`, `p`, `c`, `r`, `z`, `m=0|1`, `q=0|1|2`, `d=...`）。
   - 若 `t` 为 `f`、`t` 或 `s`，立即拒绝并根据 `q < 2` 返回 `\x1b_Gi=<id>;EINVAL:file transmission disabled\x1b\\`。
   - 支持 `m=1` / `m=0` 分块累积，解码 base64、可选 `flate2::read::ZlibDecoder` 解压，以及 `f=100`（`png::Decoder`）、`f=32`（RGBA）、`f=24`（RGB 转 RGBA）。
   - 执行尺寸检查（`width_px <= 4096 && height_px <= 4096`）与 32 MiB LRU 淘汰。
   - 在 `a=T` 或 `a=p` 时按当前光标绝对行（`history_size + cursor_y`）和列记录 `ImagePlacement`，计算在当前视口偏移下的可见区域并返回 `Vec<ViewportImage>`。
2. 将 `KittyGraphicsState` 接入 `ClientTerm`（提供 `visible_images(&self)`）与 `SessionGrid`（将 `APC G` 应答推入 `pty_replies`，并在 `queries.ts` 中吞掉客户端 APC 序列）。
3. 在 `crates/cloud-ffi/src/lib.rs` 导出 `FfiViewportImage` 与 `TermEngine::visible_images`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/ crates/cloud-ffi/ crates/server/ clients/web/src/queries.ts
git commit -m "feat(graphics): implement pure-Rust Kitty Graphics Protocol (APC G) in client-core and server"
```

---

### Task 9: 在 Desktop Native 与 Android 渲染层接入花式下划线、超链接点击与 Kitty 图片图层

**Files:**
- Modify: `clients/desktop-native/src/terminal/element.rs`
- Modify: `clients/desktop-native/src/terminal/view.rs`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/AlacrittyTerm.kt`
- Modify: `clients/android/app/src/main/java/ai/eclosion/gateterm/android/term/TerminalView.kt`

**Interfaces:**
- Consumes: `ClientTerm::visible_images()`, `ClientTerm::hyperlink_at()`, `attr::{DOUBLE_UNDERLINE, UNDERCURL, DOTTED_UNDERLINE, DASHED_UNDERLINE, HYPERLINK}` (from Tasks 7 & 8)
- Produces:
  - Desktop Native `TerminalElement` paints `UNDERCURL` / `DOUBLE_UNDERLINE` / `DOTTED` / `DASHED` underlines and `ViewportImage` RGBA overlays; `Ctrl+Click` / `Cmd+Click` opens validated `hyperlink_at(col, row)` URLs.
  - Android `TerminalView` paints `FfiViewportImage` bitmaps over the terminal canvas and renders underline variants.

- [ ] **Step 1: 在 `clients/desktop-native/src/terminal/element.rs` 编写几何与下划线辅助测试**

在 `clients/desktop-native/src/terminal/element.rs` 的 `#[cfg(test)] mod tests` 中添加测试，验证 `ViewportImage` 的单元格坐标到像素 `Bounds<Pixels>` 的映射以及波浪下划线（`UNDERCURL`）的段生成：

```rust
    #[test]
    fn computes_image_pixel_bounds_from_viewport_cells() {
        let bounds = image_cell_bounds(px(10.0), px(20.0), px(8.0), px(16.0), 2, 1, 4, 2);
        assert_eq!(bounds.origin.x, px(26.0));
        assert_eq!(bounds.origin.y, px(36.0));
        assert_eq!(bounds.size.width, px(32.0));
        assert_eq!(bounds.size.height, px(32.0));
    }
```

- [ ] **Step 2: 运行 Desktop Native 测试确认失败**

Run: `cargo test --manifest-path clients/desktop-native/Cargo.toml computes_image_pixel_bounds`
Expected: FAIL (`cannot find function image_cell_bounds`)

- [ ] **Step 3: 实现 Desktop Native 与 Android 的富属性及图片图层绘制**

1. 在 `clients/desktop-native/src/terminal/element.rs` 中：
   - 实现 `image_cell_bounds` 辅助函数，并在 `paint` 中根据 `z_index < 0`（文本下方）与 `z_index >= 0`（文本上方）绘制 `term.visible_images()`。
   - 根据 `cell.attrs` 中的 `DOUBLE_UNDERLINE`、`UNDERCURL`、`DOTTED_UNDERLINE`、`DASHED_UNDERLINE` 绘制对应样式的下划线。
2. 在 `clients/desktop-native/src/terminal/view.rs` 中：
   - 在 `on_mouse_down` 中检测 `modifiers.secondary()`（`Cmd` / `Ctrl`）点击：若 `self.term.hyperlink_at(col, row)` 返回 `Some(url)`，调用系统浏览器打开。
   - 在 `write()` 后调用 `self.term.drain_clipboard_events()` 将 `ClipboardEvent::Write` 写入 GPUI 剪贴板。
3. 在 `clients/android/.../AlacrittyTerm.kt` 与 `TerminalView.kt` 中：
   - 暴露 `visibleImages()` 与 `drainClipboardEvents()`，在 `TerminalView` 的 `Canvas` 中按单元格网格区域绘制 `Bitmap` 图层。

- [ ] **Step 4: 运行全量 Rust、Desktop Native 与 Web 测试**

Run: `cargo test --workspace && cargo test --manifest-path clients/desktop-native/Cargo.toml && npm --prefix clients/web test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add clients/desktop-native/ clients/android/
git commit -m "feat(ui): render Kitty graphics layers, fancy underlines, and OSC 8 links in desktop and Android"
```
