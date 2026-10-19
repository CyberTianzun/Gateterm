use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, Handler, ModifyOtherKeys, NamedColor, Processor, Rgb};

/// 终端查询一律由这里应答,客户端不应答(见 clients/web/src/queries.ts)。
///
/// ConPTY / 应用会发 DSR 等查询(如 `ESC [ 6 n`);alacritty 解析后通过
/// `Event::PtyWrite` 给出应答。必须写回 PTY,否则 Windows 上 shell 会一直
/// 卡在等光标位置,表现为"命令行没启动"。
///
/// 颜色查询(OSC 4/10/11/12 的 `?`)alacritty 只给 `Event::ColorRequest`,
/// 颜色由我们来查,见 [`SessionGrid::take_pty_replies`]。
#[derive(Clone)]
struct ReplyListener {
    pending: Arc<Mutex<Vec<PendingReply>>>,
}

enum PendingReply {
    Text(String),
    Color(usize, Arc<dyn Fn(Rgb) -> String + Sync + Send>),
}

impl EventListener for ReplyListener {
    fn send_event(&self, event: Event) {
        let reply = match event {
            Event::PtyWrite(text) => PendingReply::Text(text),
            Event::ColorRequest(index, format) => PendingReply::Color(index, format),
            _ => return,
        };
        self.pending.lock().unwrap().push(reply);
    }
}

/// alacritty 不实现 XTMODKEYS,Term 收到 `CSI > 4 ; Pv m` 就丢了。这里单独
/// 记下来,好让 resync 重绘把它带回去:浏览器端的 xterm.js 靠它决定
/// Shift+Enter 之类的按键编码(见 clients/web/src/keys.ts)。
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

    /// RIS(`ESC c`)。
    fn reset_state(&mut self) {
        *self = Self::default();
    }
}

pub struct SessionGrid {
    term: Term<ReplyListener>,
    parser: Processor,
    /// 第二个解析器只喂 KeyModes。Handler 有几十个方法,包一层转发给 Term
    /// 不值得;多解析一遍的开销相对 PTY 吞吐可以忽略。
    key_modes: KeyModes,
    key_modes_parser: Processor,
    cols: u16,
    rows: u16,
    pty_replies: Arc<Mutex<Vec<PendingReply>>>,
    /// 客户端报来的默认前景/背景色;没报过就不回答 OSC 10/11,让程序自己超时。
    default_colors: Option<(Rgb, Rgb)>,
}

impl SessionGrid {
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = TermSize::new(cols as usize, rows as usize);
        let pty_replies = Arc::new(Mutex::new(Vec::new()));
        let term = Term::new(
            Config::default(),
            &size,
            ReplyListener { pending: pty_replies.clone() },
        );
        Self {
            term,
            parser: Processor::new(),
            key_modes: KeyModes::default(),
            key_modes_parser: Processor::new(),
            cols,
            rows,
            pty_replies,
            default_colors: None,
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.key_modes_parser.advance(&mut self.key_modes, bytes);
    }

    /// 取走累积的查询应答,按查询到达的顺序拼好。
    ///
    /// 颜色优先用程序自己设过的(OSC 4/10/11 设色),其次是客户端报来的默认
    /// 前景/背景。都没有就不回答:编一个颜色会让程序选错深浅主题,不回答它只是
    /// 多等一会儿然后用自己的缺省。
    pub fn take_pty_replies(&self) -> Vec<u8> {
        let pending = std::mem::take(&mut *self.pty_replies.lock().unwrap());
        let mut out = String::new();
        for reply in pending {
            match reply {
                PendingReply::Text(text) => out.push_str(&text),
                PendingReply::Color(index, format) => {
                    if let Some(rgb) = self.query_color(index) {
                        out.push_str(&format(rgb));
                    }
                }
            }
        }
        out.into_bytes()
    }

    fn query_color(&self, index: usize) -> Option<Rgb> {
        if let Some(rgb) = self.term.colors()[index] {
            return Some(rgb);
        }
        let (fg, bg) = self.default_colors?;
        match index {
            i if i == NamedColor::Foreground as usize => Some(fg),
            i if i == NamedColor::Background as usize => Some(bg),
            _ => None,
        }
    }

    pub fn set_default_colors(&mut self, fg: Rgb, bg: Rgb) {
        self.default_colors = Some((fg, bg));
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.term.resize(TermSize::new(cols as usize, rows as usize));
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// SGR 相关的 flags 子集：用于测试比对，避免因 WRAPLINE/WIDE_CHAR 等
    /// 排版记账位在 roundtrip 前后合理不同而导致误判。
    #[cfg(test)]
    const SGR_FLAGS: Flags = Flags::BOLD
        .union(Flags::DIM)
        .union(Flags::ITALIC)
        .union(Flags::UNDERLINE)
        .union(Flags::INVERSE)
        .union(Flags::STRIKEOUT)
        .union(Flags::HIDDEN);

    /// 返回 (字符, "fg|bg|flags" 调试串) 供测试比对
    #[cfg(test)]
    fn cell_at(&self, line: usize, col: usize) -> (char, String) {
        let cell = &self.term.grid()[Line(line as i32)][Column(col)];
        (
            cell.c,
            format!("{:?}|{:?}|{:?}", cell.fg, cell.bg, cell.flags & Self::SGR_FLAGS),
        )
    }

    #[cfg(test)]
    fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    pub fn repaint(&self) -> Vec<u8> {
        let mut out = String::from("\x1b[0m\x1b[?25l\x1b[2J\x1b[H");
        let grid = self.term.grid();
        let mut last_sgr = String::new();
        for line in 0..self.rows as usize {
            out.push_str(&format!("\x1b[{};1H", line + 1));
            for col in 0..self.cols as usize {
                let cell = &grid[Line(line as i32)][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let sgr = sgr_string(cell.fg, cell.bg, cell.flags);
                if sgr != last_sgr {
                    out.push_str(&sgr);
                    last_sgr = sgr;
                }
                out.push(if cell.c == '\0' { ' ' } else { cell.c });
            }
        }
        out.push_str("\x1b[0m");
        let cursor = grid.cursor.point;
        out.push_str(&format!("\x1b[{};{}H", cursor.line.0 + 1, cursor.column.0 + 1));
        if self.term.mode().contains(TermMode::SHOW_CURSOR) {
            out.push_str("\x1b[?25h");
        }
        // 恢复常用终端模式,让重连后的应用(vim/less/tmux 等)不必自己重新
        // 设置就能继续正常工作。v1 已知限制:不恢复 alt-screen 与滚动区域
        // (DECSTBM)——见 README「已知限制」一节,应用可在弱网重连后按
        // Ctrl-L 或自行触发重绘来兜底。
        let mode = self.term.mode();
        if mode.contains(TermMode::APP_CURSOR) {
            out.push_str("\x1b[?1h");
        }
        if mode.contains(TermMode::APP_KEYPAD) {
            out.push_str("\x1b=");
        }
        if mode.contains(TermMode::BRACKETED_PASTE) {
            out.push_str("\x1b[?2004h");
        }
        if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            out.push_str("\x1b[?1000h");
        }
        if mode.contains(TermMode::MOUSE_DRAG) {
            out.push_str("\x1b[?1002h");
        }
        if mode.contains(TermMode::MOUSE_MOTION) {
            out.push_str("\x1b[?1003h");
        }
        if mode.contains(TermMode::SGR_MOUSE) {
            out.push_str("\x1b[?1006h");
        }
        if mode.contains(TermMode::UTF8_MOUSE) {
            out.push_str("\x1b[?1005h");
        }
        if self.key_modes.modify_other_keys != 0 {
            out.push_str(&format!("\x1b[>4;{}m", self.key_modes.modify_other_keys));
        }
        out.into_bytes()
    }
}

fn base_index(n: NamedColor) -> Option<(u8, bool)> {
    use NamedColor::*;
    Some(match n {
        Black => (0, false), Red => (1, false), Green => (2, false), Yellow => (3, false),
        Blue => (4, false), Magenta => (5, false), Cyan => (6, false), White => (7, false),
        BrightBlack => (0, true), BrightRed => (1, true), BrightGreen => (2, true),
        BrightYellow => (3, true), BrightBlue => (4, true), BrightMagenta => (5, true),
        BrightCyan => (6, true), BrightWhite => (7, true),
        _ => return None,
    })
}

fn color_params(c: Color, fg: bool) -> String {
    let (default, base, bright_base, ext) = if fg { ("39", 30, 90, "38") } else { ("49", 40, 100, "48") };
    match c {
        Color::Named(n) => match base_index(n) {
            Some((i, false)) => format!("{}", base + i as u16),
            Some((i, true)) => format!("{}", bright_base + i as u16),
            None => default.to_string(),
        },
        Color::Indexed(i) => format!("{ext};5;{i}"),
        Color::Spec(rgb) => format!("{ext};2;{};{};{}", rgb.r, rgb.g, rgb.b),
    }
}

fn sgr_string(fg: Color, bg: Color, flags: Flags) -> String {
    let mut parts = vec!["0".to_string()];
    if flags.contains(Flags::BOLD) { parts.push("1".into()); }
    if flags.contains(Flags::DIM) { parts.push("2".into()); }
    if flags.contains(Flags::ITALIC) { parts.push("3".into()); }
    // v1 起见,把所有花式下划线变体(双线/曲线/点线/虚线)都近似成普通下划线 SGR 4。
    if flags.intersects(
        Flags::UNDERLINE
            | Flags::DOUBLE_UNDERLINE
            | Flags::UNDERCURL
            | Flags::DOTTED_UNDERLINE
            | Flags::DASHED_UNDERLINE,
    ) {
        parts.push("4".into());
    }
    if flags.contains(Flags::INVERSE) { parts.push("7".into()); }
    if flags.contains(Flags::STRIKEOUT) { parts.push("9".into()); }
    if flags.contains(Flags::HIDDEN) { parts.push("8".into()); }
    parts.push(color_params(fg, true));
    parts.push(color_params(bg, false));
    format!("\x1b[{}m", parts.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsr_cursor_query_produces_pty_reply() {
        let mut g = SessionGrid::new(80, 24);
        g.advance(b"\x1b[6n");
        let reply = String::from_utf8(g.take_pty_replies()).unwrap();
        assert!(
            reply.contains("\x1b[") && reply.contains('R'),
            "expected CPR reply like ESC[row;colR, got {reply:?}"
        );
    }

    #[test]
    fn background_query_is_unanswered_until_client_reports_colors() {
        let mut g = SessionGrid::new(80, 24);
        g.advance(b"\x1b]11;?\x1b\\");
        assert!(g.take_pty_replies().is_empty());

        g.set_default_colors(Rgb { r: 0xd4, g: 0xd4, b: 0xd4 }, Rgb { r: 0x19, g: 0x1a, b: 0x1b });
        g.advance(b"\x1b]11;?\x1b\\");
        assert_eq!(
            String::from_utf8(g.take_pty_replies()).unwrap(),
            "\x1b]11;rgb:1919/1a1a/1b1b\x1b\\"
        );
        g.advance(b"\x1b]10;?\x07");
        assert_eq!(
            String::from_utf8(g.take_pty_replies()).unwrap(),
            "\x1b]10;rgb:d4d4/d4d4/d4d4\x07"
        );
    }

    #[test]
    fn color_set_by_the_program_wins_over_client_default() {
        let mut g = SessionGrid::new(80, 24);
        g.set_default_colors(Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 0, g: 0, b: 0 });
        g.advance(b"\x1b]11;#ffffff\x07\x1b]11;?\x07");
        assert_eq!(
            String::from_utf8(g.take_pty_replies()).unwrap(),
            "\x1b]11;rgb:ffff/ffff/ffff\x07"
        );
    }

    #[test]
    fn replies_keep_query_order() {
        let mut g = SessionGrid::new(80, 24);
        g.set_default_colors(Rgb { r: 1, g: 2, b: 3 }, Rgb { r: 4, g: 5, b: 6 });
        g.advance(b"\x1b]11;?\x07\x1b[6n");
        assert_eq!(
            String::from_utf8(g.take_pty_replies()).unwrap(),
            "\x1b]11;rgb:0404/0505/0606\x07\x1b[1;1R"
        );
    }

    #[test]
    fn plain_text_lands_in_grid() {
        let mut g = SessionGrid::new(20, 5);
        g.advance(b"hi");
        assert_eq!(g.cell_at(0, 0).0, 'h');
        assert_eq!(g.cell_at(0, 1).0, 'i');
    }

    #[test]
    fn repaint_roundtrip_preserves_content_and_color() {
        let mut a = SessionGrid::new(20, 5);
        a.advance(b"plain \x1b[31mred\x1b[0m\r\nline2 \x1b[1;44mbold-on-blue\x1b[0m\x1b[8mhidden");
        let repaint = a.repaint();

        let mut b = SessionGrid::new(20, 5);
        b.advance(&repaint);

        for line in 0..5 {
            for col in 0..20 {
                assert_eq!(
                    a.cell_at(line, col),
                    b.cell_at(line, col),
                    "cell mismatch at {line},{col}"
                );
            }
        }
    }

    #[test]
    fn repaint_roundtrip_preserves_wide_chars() {
        let mut a = SessionGrid::new(20, 5);
        // "你好 ok" 中文宽字符 + 红色前景，行尾要覆盖 WIDE_CHAR_SPACER 跳过路径与列对齐。
        a.advance(b"\x1b[31m\xe4\xbd\xa0\xe5\xa5\xbd ok");
        let repaint = a.repaint();

        let mut b = SessionGrid::new(20, 5);
        b.advance(&repaint);

        for line in 0..5 {
            for col in 0..20 {
                assert_eq!(
                    a.cell_at(line, col),
                    b.cell_at(line, col),
                    "cell mismatch at {line},{col}"
                );
            }
        }
    }

    #[test]
    fn repaint_restores_cursor_position() {
        let mut a = SessionGrid::new(20, 5);
        a.advance(b"abc\x1b[3;4H"); // 光标移到 3 行 4 列
        let repaint = String::from_utf8(a.repaint()).unwrap();
        assert!(repaint.ends_with("\x1b[3;4H\x1b[?25h"), "got tail: {:?}", &repaint[repaint.len().saturating_sub(24)..]);
    }

    #[test]
    fn repaint_restores_terminal_modes() {
        let mut a = SessionGrid::new(20, 5);
        // 括号粘贴 + 应用光标键
        a.advance(b"\x1b[?2004h\x1b[?1h");
        assert!(a.mode().contains(TermMode::BRACKETED_PASTE));
        assert!(a.mode().contains(TermMode::APP_CURSOR));
        let repaint = a.repaint();

        let mut b = SessionGrid::new(20, 5);
        b.advance(&repaint);

        assert!(b.mode().contains(TermMode::BRACKETED_PASTE), "mode: {:?}", b.mode());
        assert!(b.mode().contains(TermMode::APP_CURSOR), "mode: {:?}", b.mode());
    }

    #[test]
    fn repaint_restores_modify_other_keys() {
        let mut a = SessionGrid::new(20, 5);
        assert!(!String::from_utf8(a.repaint()).unwrap().contains("\x1b[>4;"));
        a.advance(b"\x1b[>4;2m");
        let mut b = SessionGrid::new(20, 5);
        b.advance(&a.repaint());
        assert_eq!(b.key_modes.modify_other_keys, 2);
        a.advance(b"\x1b[>4;1m");
        assert!(String::from_utf8(a.repaint()).unwrap().contains("\x1b[>4;1m"));
        // 省略 Pv 即复位;RIS 也复位
        a.advance(b"\x1b[>4m");
        assert_eq!(a.key_modes.modify_other_keys, 0);
        a.advance(b"\x1b[>4;2m\x1bc");
        assert_eq!(a.key_modes.modify_other_keys, 0);
        // 跨 read 边界切开的序列也能识别
        a.advance(b"\x1b[>4");
        a.advance(b";2m");
        assert_eq!(a.key_modes.modify_other_keys, 2);
    }

    #[test]
    fn repaint_restores_mouse_reporting_modes() {
        let mut a = SessionGrid::new(20, 5);
        // SGR 扩展坐标 + 任意动作鼠标上报(隐含 click/drag/motion)
        a.advance(b"\x1b[?1006h\x1b[?1003h");
        let repaint = a.repaint();

        let mut b = SessionGrid::new(20, 5);
        b.advance(&repaint);

        assert!(b.mode().contains(TermMode::SGR_MOUSE), "mode: {:?}", b.mode());
        assert!(b.mode().contains(TermMode::MOUSE_MOTION), "mode: {:?}", b.mode());
    }
}
