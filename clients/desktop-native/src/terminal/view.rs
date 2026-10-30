//! `TerminalView` GPUI entity: wraps `gateterm_client_core::term::ClientTerm`,
//! manages viewport selection, scrollback, mouse tracking, clipboard copy/paste,
//! and channel input/resize callbacks.

use std::ops::Range;
use std::rc::Rc;

use gateterm_client_core::term::ClientTerm;
use gpui::{
    App, Bounds, ClipboardEntry, ClipboardItem, Context, EntityInputHandler, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Styled as _, UTF16Selection, Window, div, point, px,
};
use gpui_component::ActiveTheme as _;

use super::element::TerminalElement;
use super::input::encode_keystroke;
use crate::client::api::GateHttpClient;
use crate::config::LOCAL_SERVER_ID;
use crate::gpui_tokio::Tokio;

pub type InputCallback = Rc<dyn Fn(&[u8], &mut App)>;
pub type ResizeCallback = Rc<dyn Fn(u16, u16, &mut App)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPoint {
    pub col: i32,
    pub row: i32,
}

pub struct TerminalView {
    pub server_id: String,
    pub session_id: u64,
    pub session_name: String,
    pub channel: u32,
    pub server_os: String,
    pub win32_input_mode: bool,
    pub term: ClientTerm,
    pub focus_handle: FocusHandle,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub line_height_mul: f32,
    pub ime_marked_text: Option<String>,
    pub exited: bool,

    // Layout metrics updated during prepaint
    viewport_origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    requested_cols: u16,
    requested_rows: u16,

    // Selection in viewport coordinates
    selection_anchor: Option<GridPoint>,
    selection_head: Option<GridPoint>,
    dragging_selection: bool,

    // In-terminal scrollback search matches: (abs_row, col_start, col_end_exclusive)
    pub search_matches: Vec<(usize, usize, usize)>,
    pub search_current_idx: Option<usize>,

    on_input: Option<InputCallback>,
    on_resize: Option<ResizeCallback>,
    http_client: Option<GateHttpClient>,
}

impl TerminalView {
    pub fn new(
        server_id: String,
        session_id: u64,
        session_name: String,
        channel: u32,
        server_os: String,
        win32_input_mode: bool,
        font_family: impl Into<SharedString>,
        font_size: f32,
        line_height_mul: f32,
        max_scrollback: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            server_id,
            session_id,
            session_name,
            channel,
            server_os,
            win32_input_mode,
            term: ClientTerm::new(80, 24, max_scrollback),
            focus_handle: cx.focus_handle(),
            font_family: font_family.into(),
            font_size: px(font_size),
            line_height_mul,
            ime_marked_text: None,
            exited: false,
            viewport_origin: point(px(0.0), px(0.0)),
            cell_width: px(8.4),
            line_height: px(18.0),
            requested_cols: 80,
            requested_rows: 24,
            selection_anchor: None,
            selection_head: None,
            dragging_selection: false,
            search_matches: Vec::new(),
            search_current_idx: None,
            on_input: None,
            on_resize: None,
            http_client: None,
        }
    }

    pub fn with_callbacks(
        mut self,
        on_input: InputCallback,
        on_resize: ResizeCallback,
    ) -> Self {
        self.on_input = Some(on_input);
        self.on_resize = Some(on_resize);
        self
    }

    pub fn with_http_client(mut self, http_client: Option<GateHttpClient>) -> Self {
        self.http_client = http_client;
        self
    }

    pub fn write_output(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.term.write(bytes);
        cx.notify();
    }

    pub fn write_pty_bytes(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.write_output(bytes, cx);
    }

    pub fn mark_exited(&mut self, cx: &mut Context<Self>) {
        self.exited = true;
        cx.notify();
    }

    pub fn dimensions(&self) -> (u16, u16) {
        self.desired_size()
    }

    pub fn set_font(
        &mut self,
        font_family: impl Into<SharedString>,
        font_size: f32,
        line_height_mul: f32,
        cx: &mut Context<Self>,
    ) {
        self.font_family = font_family.into();
        self.font_size = px(font_size);
        self.line_height_mul = line_height_mul;
        cx.notify();
    }

    pub fn reset_terminal(&mut self, cx: &mut Context<Self>) {
        self.term.reset();
        self.clear_selection();
        cx.notify();
    }

    pub fn apply_authoritative_resize(&mut self, cols: u16, rows: u16, cx: &mut Context<Self>) {
        self.term.resize(cols, rows);
        cx.notify();
    }

    pub fn update_appearance(
        &mut self,
        font_family: impl Into<SharedString>,
        font_size: f32,
        line_height_mul: f32,
        max_scrollback: usize,
        cx: &mut Context<Self>,
    ) {
        self.font_family = font_family.into();
        self.font_size = px(font_size);
        self.line_height_mul = line_height_mul;
        self.term.set_max_scrollback(max_scrollback);
        cx.notify();
    }

    pub fn desired_size(&self) -> (u16, u16) {
        (self.requested_cols, self.requested_rows)
    }

    pub(crate) fn update_viewport_geometry(
        &mut self,
        origin: Point<Pixels>,
        cell_width: Pixels,
        line_height: Pixels,
        cols: u16,
        rows: u16,
        cx: &mut Context<Self>,
    ) {
        self.viewport_origin = origin;
        self.cell_width = cell_width;
        self.line_height = line_height;
        if cols != self.requested_cols || rows != self.requested_rows {
            self.requested_cols = cols;
            self.requested_rows = rows;
            // Keep parsing at the shared PTY size until the server sends Resized.
            // Another client's viewport may determine a different authoritative size.
            if let Some(cb) = self.on_resize.clone() {
                cb(cols, rows, cx);
            }
        }
    }

    pub fn send_bytes(&self, bytes: &[u8], cx: &mut App) {
        if !bytes.is_empty()
            && let Some(cb) = &self.on_input
        {
            cb(bytes, cx);
        }
    }

    pub fn has_selection(&self) -> bool {
        match (self.selection_anchor, self.selection_head) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_head = None;
        self.dragging_selection = false;
    }

    pub fn is_cell_selected(&self, col: i32, row: i32) -> bool {
        let (Some(a), Some(b)) = (self.selection_anchor, self.selection_head) else {
            return false;
        };
        if a == b {
            return false;
        }
        let (start, end) = if (a.row, a.col) <= (b.row, b.col) {
            (a, b)
        } else {
            (b, a)
        };
        if row < start.row || row > end.row {
            return false;
        }
        if start.row == end.row {
            col >= start.col && col <= end.col
        } else if row == start.row {
            col >= start.col
        } else if row == end.row {
            col <= end.col
        } else {
            true
        }
    }

    pub fn update_search_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.search_matches = self.term.search_substring(query);
        if self.search_matches.is_empty() {
            self.search_current_idx = None;
        } else {
            // Default to the latest (bottom-most) match
            let last_idx = self.search_matches.len() - 1;
            self.search_current_idx = Some(last_idx);
            self.scroll_to_search_match(last_idx);
        }
        cx.notify();
    }

    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.search_matches.clear();
        self.search_current_idx = None;
        cx.notify();
    }

    pub fn step_search_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.search_matches.len();
        if count == 0 {
            self.search_current_idx = None;
            return;
        }
        let next = match self.search_current_idx {
            Some(cur) => {
                if forward {
                    (cur + 1) % count
                } else if cur == 0 {
                    count - 1
                } else {
                    cur - 1
                }
            }
            None => count - 1,
        };
        self.search_current_idx = Some(next);
        self.scroll_to_search_match(next);
        cx.notify();
    }

    fn scroll_to_search_match(&mut self, idx: usize) {
        let Some(&(abs_row, _, _)) = self.search_matches.get(idx) else {
            return;
        };
        let st = self.term.state();
        let hist = st.max_scroll_offset as usize;
        let rows = st.rows as usize;
        let total = hist + rows;
        if abs_row >= hist {
            self.term.scroll_to_bottom();
        } else {
            // Center the matched line in the viewport
            let half = rows / 2;
            let bottom_abs = (abs_row + half).min(total.saturating_sub(1));
            let target_offset = total.saturating_sub(1).saturating_sub(bottom_abs);
            self.term.scroll_to_offset(target_offset);
        }
    }

    /// Returns `Some(true)` if cell `(col, row)` in current viewport is part of the active search match,
    /// `Some(false)` if part of another search match, or `None` if not matched.
    pub fn cell_search_match_kind(&self, col: usize, row: usize) -> Option<bool> {
        if self.search_matches.is_empty() {
            return None;
        }
        let st = self.term.state();
        let hist = st.max_scroll_offset as usize;
        let off = st.scroll_offset as usize;
        let abs_row = (hist + row).saturating_sub(off);
        for (i, &(m_row, c0, c1)) in self.search_matches.iter().enumerate() {
            if m_row == abs_row && col >= c0 && col < c1 {
                return Some(self.search_current_idx == Some(i));
            }
        }
        None
    }

    pub fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(a), Some(b)) = (self.selection_anchor, self.selection_head) else {
            return false;
        };
        if a == b {
            return false;
        }
        let text = self.term.copy_viewport(a.col, a.row, b.col, b.row);
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.clear_selection();
            cx.notify();
            return true;
        }
        false
    }

    pub fn paste_text_str(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.term.scroll_to_bottom();
        let encoded = self.term.encode_paste(text);
        self.send_bytes(&encoded, cx);
        cx.notify();
    }

    pub fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };

        // 1. Copied local files from Explorer / Finder
        let paths: Vec<std::path::PathBuf> = item
            .entries()
            .iter()
            .filter_map(|e| match e {
                ClipboardEntry::ExternalPaths(p) => Some(p.paths()),
                _ => None,
            })
            .flatten()
            .cloned()
            .collect();

        if !paths.is_empty() {
            if self.server_id == LOCAL_SERVER_ID {
                let formatted = format_paths_for_paste(
                    &paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                );
                self.paste_text_str(&formatted, cx);
                return;
            } else if let Some(http) = self.http_client.clone() {
                // If user copied image files and pasted into a remote session, upload them
                let img_files: Vec<std::path::PathBuf> = paths
                    .into_iter()
                    .filter(|p| p.is_file())
                    .collect();
                if !img_files.is_empty() {
                    let task = Tokio::spawn_result(cx, async move {
                        let mut remote_paths = Vec::new();
                        for p in img_files {
                            if let Ok(bytes) = std::fs::read(&p)
                                && gateterm_server::app::detect_image_extension(&bytes).is_some()
                                && let Ok(remote) = http.upload_pasted_image(bytes).await
                            {
                                remote_paths.push(remote);
                            }
                        }
                        Ok(remote_paths)
                    });
                    cx.spawn(async move |this, cx| {
                        if let Ok(remote_paths) = task.await
                            && !remote_paths.is_empty()
                        {
                            let formatted = format_paths_for_paste(&remote_paths);
                            let _ = this.update(cx, |this, cx| {
                                this.paste_text_str(&formatted, cx);
                            });
                        }
                    })
                    .detach();
                    return;
                }
            }
        }

        // 2. Plain text on clipboard
        if let Some(text) = item.text()
            && !text.is_empty()
        {
            self.paste_text_str(&text, cx);
            return;
        }

        // 3. Screenshot / image data on clipboard (`Win+Shift+S`, `Cmd+Ctrl+Shift+4`, etc.)
        if let Some(img) = item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(img) => Some(img),
            _ => None,
        }) {
            let bytes = img.bytes.clone();
            if self.server_id == LOCAL_SERVER_ID {
                let dir = std::env::temp_dir().join("gateterm-paste");
                if let Ok(path) = gateterm_server::app::save_pasted_image_bytes(&dir, &bytes) {
                    let formatted = format_paths_for_paste(&[path.to_string_lossy().into_owned()]);
                    self.paste_text_str(&formatted, cx);
                }
            } else if let Some(http) = self.http_client.clone() {
                let task =
                    Tokio::spawn_result(cx, async move { http.upload_pasted_image(bytes).await });
                cx.spawn(async move |this, cx| {
                    if let Ok(remote_path) = task.await {
                        let formatted = format_paths_for_paste(&[remote_path]);
                        let _ = this.update(cx, |this, cx| {
                            this.paste_text_str(&formatted, cx);
                        });
                    }
                })
                .detach();
            }
        }
    }

    fn point_to_grid(&self, position: Point<Pixels>) -> GridPoint {
        let st = self.term.state();
        let rel_x = (position.x - self.viewport_origin.x).as_f32();
        let rel_y = (position.y - self.viewport_origin.y).as_f32();
        let col = (rel_x / self.cell_width.as_f32().max(1.0))
            .floor()
            .clamp(0.0, (st.cols.saturating_sub(1)) as f32) as i32;
        let row = (rel_y / self.line_height.as_f32().max(1.0))
            .floor()
            .clamp(0.0, (st.rows.saturating_sub(1)) as f32) as i32;
        GridPoint { col, row }
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Never intercept keys during active IME composition (e.g., Enter, Space, Backspace,
        // Escape, or arrow keys used for candidate selection).
        if self.ime_marked_text.is_some() {
            return;
        }

        let ks = &ev.keystroke;
        let m = &ks.modifiers;

        // Copy / Paste shortcuts (aligned with clients/web/src/keys.ts `browserShortcut`):
        // - Copy: Cmd+C, Ctrl+Shift+C, Ctrl+Insert, or Ctrl+C when terminal has an active selection
        // - Paste: Cmd+V, Ctrl+V, Ctrl+Shift+V, or Shift+Insert
        let key_lower = ks.key.to_ascii_lowercase();
        let is_copy = (m.platform && !m.control && !m.alt && key_lower == "c")
            || (m.control && !m.alt && key_lower == "c" && (m.shift || self.has_selection()))
            || (m.control && !m.shift && !m.alt && key_lower == "insert");
        if is_copy {
            let _ = self.copy_selection(cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        let is_paste = (m.platform && !m.control && !m.alt && key_lower == "v")
            || (m.control && !m.alt && key_lower == "v")
            || (m.shift && !m.control && !m.alt && key_lower == "insert");
        if is_paste {
            self.paste_from_clipboard(cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        // Scrollback navigation via Shift+PageUp / Shift+PageDown when not in win32-input-mode
        // and scrollback history exists; otherwise forward to the terminal/agent.
        if !self.win32_input_mode
            && self.term.state().max_scroll_offset > 0
            && m.shift
            && !m.control
            && !m.alt
        {
            if key_lower == "pageup" {
                let page = (self.term.state().rows as i32 / 2).max(1);
                self.term.scroll_by(page);
                cx.notify();
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
            if key_lower == "pagedown" {
                let page = (self.term.state().rows as i32 / 2).max(1);
                self.term.scroll_by(-page);
                cx.notify();
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
        }

        let state = self.term.state();
        if let Some(bytes) = encode_keystroke(
            ks,
            &state,
            &self.server_os,
            self.has_selection(),
            self.win32_input_mode,
        ) {
            self.term.scroll_to_bottom();
            self.clear_selection();
            self.send_bytes(&bytes, cx);
            cx.notify();
            window.prevent_default();
            cx.stop_propagation();
        }
    }

    fn on_mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let pt = self.point_to_grid(ev.position);
        let btn_code = match ev.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return,
        };

        if !ev.modifiers.shift
            && let Some(bytes) = self.term.encode_pointer(pt.col, pt.row, btn_code, true, false)
        {
            self.send_bytes(&bytes, cx);
            return;
        }

        if ev.button == MouseButton::Left {
            self.selection_anchor = Some(pt);
            self.selection_head = Some(pt);
            self.dragging_selection = true;
            cx.notify();
        } else if ev.button == MouseButton::Right {
            if self.has_selection() {
                self.copy_selection(cx);
            } else {
                self.paste_from_clipboard(cx);
            }
        }
    }

    fn on_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pt = self.point_to_grid(ev.position);
        let pressed = ev.pressed_button.is_some();
        let btn_code = match ev.pressed_button {
            Some(MouseButton::Left) | None => 0,
            Some(MouseButton::Middle) => 1,
            Some(MouseButton::Right) => 2,
            _ => 0,
        };

        if !ev.modifiers.shift
            && let Some(bytes) = self.term.encode_pointer(pt.col, pt.row, btn_code, pressed, true)
        {
            self.send_bytes(&bytes, cx);
            return;
        }

        if self.dragging_selection && ev.pressed_button == Some(MouseButton::Left) {
            if self.selection_head != Some(pt) {
                self.selection_head = Some(pt);
                cx.notify();
            }
        }
    }

    fn on_mouse_up(
        &mut self,
        ev: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pt = self.point_to_grid(ev.position);
        let btn_code = match ev.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return,
        };

        if !ev.modifiers.shift
            && let Some(bytes) = self.term.encode_pointer(pt.col, pt.row, btn_code, false, false)
        {
            self.send_bytes(&bytes, cx);
            return;
        }

        if ev.button == MouseButton::Left {
            self.dragging_selection = false;
            cx.notify();
        }
    }

    fn on_scroll_wheel(
        &mut self,
        ev: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pixel_delta = ev.delta.pixel_delta(self.line_height);
        let lines = (pixel_delta.y.as_f32() / self.line_height.as_f32().max(1.0)).round() as i32;
        if lines == 0 {
            return;
        }
        let st = self.term.state();
        if st.mouse_tracking && !ev.modifiers.shift {
            let pt = self.point_to_grid(ev.position);
            // Wheel up = button 64, wheel down = button 65 in SGR mouse mode
            let btn = if lines > 0 { 64 } else { 65 };
            let count = lines.unsigned_abs().min(5);
            for _ in 0..count {
                let seq = format!("\x1b[<{btn};{};{}M", pt.col + 1, pt.row + 1);
                self.send_bytes(seq.as_bytes(), cx);
            }
            return;
        }
        self.term.scroll_by(lines * 3);
        cx.notify();
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.ime_marked_text
            .as_ref()
            .map(|s| 0..s.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.ime_marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_marked_text = None;
        if !new_text.is_empty() {
            self.term.scroll_to_bottom();
            self.clear_selection();
            self.send_bytes(new_text.as_bytes(), cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let st = self.term.state();
        let scrolled = st.scroll_offset > 0;
        let exited = self.exited;
        let theme = cx.theme();

        div()
            .id(("term-pane", self.channel as usize))
            .key_context("TerminalView")
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(TerminalElement::new(view))
            .children(scrolled.then(|| {
                let offset = st.scroll_offset;
                div()
                    .absolute()
                    .top_2()
                    .right_4()
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.0))
                    .bg(theme.secondary)
                    .border_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(format!("↑ -{offset} lines"))
            }))
            .children(exited.then(|| {
                div()
                    .absolute()
                    .bottom_2()
                    .left_3()
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.0))
                    .bg(theme.danger)
                    .text_xs()
                    .text_color(theme.danger_foreground)
                    .child("[Session exited]")
            }))
    }
}

pub fn quote_path_for_shell(path: &str) -> String {
    if path.contains([' ', '\t', '"', '\'', '&', '(', ')', '[', ']', '$', ';']) {
        format!("\"{}\"", path.replace('"', "\\\""))
    } else {
        path.to_string()
    }
}

pub fn format_paths_for_paste(paths: &[String]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let words: Vec<String> = paths.iter().map(|p| quote_path_for_shell(p)).collect();
    format!("{} ", words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::cell::RefCell;

    fn terminal_view(
        cx: &mut TestAppContext,
        requests: Rc<RefCell<Vec<(u16, u16)>>>,
    ) -> gpui::Entity<TerminalView> {
        cx.new(|cx| {
            TerminalView::new(
                "test".into(),
                1,
                "test".into(),
                1,
                "linux".into(),
                false,
                "monospace",
                14.0,
                1.0,
                100,
                cx,
            )
            .with_callbacks(
                Rc::new(|_, _| {}),
                Rc::new(move |cols, rows, _| requests.borrow_mut().push((cols, rows))),
            )
        })
    }

    #[gpui::test]
    fn viewport_proposal_preserves_in_flight_output_geometry(cx: &mut TestAppContext) {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let view = terminal_view(cx, requests.clone());
        view.update(cx, |view, cx| {
            view.apply_authoritative_resize(10, 4, cx);
            view.write_output(b"123456789", cx);

            view.update_viewport_geometry(point(px(0.0), px(0.0)), px(8.0), px(16.0), 5, 4, cx);
            // These bytes were produced before the server accepted the smaller viewport.
            view.write_output(b"XY", cx);
            assert_eq!(view.term.copy_viewport(0, 0, 9, 0), "123456789X");
            assert_eq!(view.term.copy_viewport(0, 1, 0, 1), "Y");
            assert_eq!((view.term.state().cols, view.term.state().rows), (10, 4));
            assert_eq!(view.desired_size(), (5, 4));

            view.apply_authoritative_resize(5, 4, cx);
            view.write_output(b"\x1b[2J\x1b[Habcdef", cx);
            assert_eq!(view.term.copy_viewport(0, 0, 4, 0), "abcde");
            assert_eq!(view.term.copy_viewport(0, 1, 0, 1), "f");
            assert_eq!((view.term.state().cols, view.term.state().rows), (5, 4));
        });
        assert_eq!(*requests.borrow(), vec![(5, 4)]);
    }

    #[gpui::test]
    fn authoritative_smallest_client_size_does_not_replace_viewport_proposal(
        cx: &mut TestAppContext,
    ) {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let view = terminal_view(cx, requests.clone());
        view.update(cx, |view, cx| {
            view.apply_authoritative_resize(40, 12, cx);
            view.update_viewport_geometry(point(px(0.0), px(0.0)), px(8.0), px(16.0), 120, 40, cx);
            assert_eq!((view.term.state().cols, view.term.state().rows), (40, 12));
            // A second client's smaller viewport determines the shared PTY size.
            view.apply_authoritative_resize(60, 20, cx);
            assert_eq!(view.dimensions(), (120, 40));

            // Repainting or refocusing the unchanged viewport must keep that shared size.
            view.update_viewport_geometry(point(px(1.0), px(1.0)), px(8.0), px(16.0), 120, 40, cx);
            assert_eq!((view.term.state().cols, view.term.state().rows), (60, 20));
            assert_eq!(view.desired_size(), (120, 40));
        });
        assert_eq!(*requests.borrow(), vec![(120, 40)]);
    }

    #[test]
    fn formats_pasted_paths_with_quoting_when_needed() {
        assert_eq!(
            format_paths_for_paste(&["/tmp/gateterm-paste/paste-1.png".to_string()]),
            "/tmp/gateterm-paste/paste-1.png "
        );
        assert_eq!(
            format_paths_for_paste(&[r"C:\Users\My User\paste-1.png".to_string()]),
            "\"C:\\Users\\My User\\paste-1.png\" "
        );
    }
}
