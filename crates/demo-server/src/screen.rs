use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

#[derive(Clone)]
struct Quiet;
impl EventListener for Quiet {
    fn send_event(&self, _: Event) {}
}

// Output is generated entirely by the virtual editor. It contains plain text,
// CR/LF, CUP and erase sequences only; arbitrary ANSI modes cannot enter it.
pub struct Screen {
    term: Term<Quiet>,
    parser: Processor,
    cols: u16,
    rows: u16,
}
impl Screen {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            term: Term::new(
                Config {
                    scrolling_history: 0,
                    ..Config::default()
                },
                &TermSize::new(cols.into(), rows.into()),
                Quiet,
            ),
            parser: Processor::new(),
            cols,
            rows,
        }
    }
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(TermSize::new(cols.into(), rows.into()));
        self.cols = cols;
        self.rows = rows;
    }
    pub fn repaint(&self) -> Vec<u8> {
        // Workspace clamps the grid to 9600 cells and bounds combining marks
        // before this parser. Even base64-encoded preview fits one 64KiB frame.
        let mut out = String::from(
            "\x1b[0m\x1b[?1l\x1b>\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[>4;0m\x1b[2J\x1b[H",
        );
        let grid = self.term.grid();
        for row in 0..self.rows {
            out.push_str(&format!("\x1b[{};1H", row + 1));
            let mut text = String::new();
            for col in 0..self.cols {
                let cell = &grid[Line(i32::from(row))][Column(usize::from(col))];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                text.push(cell.c);
                if let Some(extra) = cell.zerowidth() {
                    for &c in extra {
                        text.push(c);
                    }
                }
            }
            out.push_str(text.trim_end_matches(' '));
        }
        let cursor = &grid.cursor;
        out.push_str(&format!(
            "\x1b[{};{}H\x1b[?25h",
            cursor.point.line.0 + 1,
            cursor.point.column.0 + 1
        ));
        // Restore deferred autowrap by rewriting the final glyph at its origin.
        if cursor.input_needs_wrap {
            let mut col = cursor.point.column.0;
            if grid[cursor.point.line][Column(col)]
                .flags
                .contains(Flags::WIDE_CHAR_SPACER)
            {
                col = col.saturating_sub(1);
            }
            let cell = &grid[cursor.point.line][Column(col)];
            out.push_str(&format!(
                "\x1b[{};{}H{}",
                cursor.point.line.0 + 1,
                col + 1,
                cell.c
            ));
            if let Some(extra) = cell.zerowidth() {
                for &c in extra {
                    out.push(c);
                }
            }
        }
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repaint_restores_screen_and_unicode_after_resize() {
        let mut screen = Screen::new(80, 24);
        screen.advance("Virtual demo\r\n你好🙂\r\nvirtual:/workspace$ ".as_bytes());
        screen.resize(40, 12);
        let repaint = screen.repaint();
        assert!(String::from_utf8_lossy(&repaint).contains("Virtual demo"));
        assert!(String::from_utf8_lossy(&repaint).contains("你好🙂"));
        let mut restored = Screen::new(40, 12);
        restored.advance(&repaint);
        assert_eq!(restored.repaint(), repaint);
    }
}
