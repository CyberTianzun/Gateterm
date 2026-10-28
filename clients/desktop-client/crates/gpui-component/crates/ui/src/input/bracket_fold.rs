//! Grammar-agnostic fold candidate extractor based on bracket balance.
//!
//! Intended to be passed to [`crate::input::InputState::set_fold_extractor`] as
//! an alternative to the default tree-sitter-based extraction. Useful for SQL
//! dialects and other languages where tree-sitter grammars produce fragmented
//! parses (ERROR nodes) and cause fold candidates to land on sub-statement
//! fragments.
//!
//! The scanner treats these as foldable regions:
//!
//! - `(…)`, `{…}`, `[…]` pairs that span at least [`MIN_FOLD_LINES`] rows.
//! - `/* … */` block comments that span at least [`MIN_FOLD_LINES`] rows.
//!
//! Strings (`'…'`, `"…"`, `` `…` ``, with `\` and doubled-quote escapes) and
//! line comments (`--`, `//`) are skipped so brackets inside them do not
//! affect nesting.

use ropey::Rope;

use crate::input::display_map::FoldRange;

/// Minimum line span between a fold's start and end for it to be offered.
///
/// A value of 2 means the opening and closing markers must be at least two
/// rows apart (i.e. at least one row is hidden when folded).
const MIN_FOLD_LINES: usize = 2;

/// Fold-candidate extractor based on bracket balance.
///
/// Designed to be passed directly to [`crate::input::InputState::set_fold_extractor`].
pub fn extract_fold_ranges(text: &Rope) -> Vec<FoldRange> {
    let text = text.to_string();
    let bytes = text.as_bytes();
    let n = bytes.len();

    let mut ranges: Vec<FoldRange> = Vec::new();
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut line: usize = 0;
    let mut i = 0;

    while i < n {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            q @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < n {
                    if bytes[i] == b'\\' && i + 1 < n {
                        if bytes[i + 1] == b'\n' {
                            line += 1;
                        }
                        i += 2;
                    } else if bytes[i] == q {
                        // SQL-style doubled-quote escape: '' or "".
                        if i + 1 < n && bytes[i + 1] == q {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        if bytes[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                }
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < n && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < n && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let start_line = line;
                i += 2;
                while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                let end_line = line;
                i = (i + 2).min(n);
                if end_line.saturating_sub(start_line) >= MIN_FOLD_LINES {
                    ranges.push(FoldRange::new(start_line, end_line));
                }
            }
            b'(' | b'{' | b'[' => {
                stack.push((bytes[i], line));
                i += 1;
            }
            b')' | b'}' | b']' => {
                let want = match bytes[i] {
                    b')' => b'(',
                    b'}' => b'{',
                    b']' => b'[',
                    _ => unreachable!(),
                };
                // Pop until a matching opener is found. Tolerates unbalanced
                // input (e.g. a partially-written script) by discarding
                // mismatched openers rather than giving up on the rest of the
                // file.
                while let Some(&(top, start_line)) = stack.last() {
                    stack.pop();
                    if top == want {
                        if line.saturating_sub(start_line) >= MIN_FOLD_LINES {
                            ranges.push(FoldRange::new(start_line, line));
                        }
                        break;
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }

    // Sort by start_line ASC, then by end_line DESC so `dedup_by_key` keeps
    // the outermost fold at each start_line — the one most useful to collapse.
    ranges.sort_by(|a, b| {
        a.start_line
            .cmp(&b.start_line)
            .then(b.end_line.cmp(&a.end_line))
    });
    ranges.dedup_by_key(|r| r.start_line);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clickhouse_ddl_folds_outer_parens() {
        let sql = "CREATE TABLE t (\n\
            `a` Int,\n\
            `b` String CODEC(ZSTD(1)),\n\
            INDEX idx a TYPE bloom_filter(0.001) GRANULARITY 1\n\
        ) ENGINE = MergeTree;\n";
        let rope = Rope::from(sql);
        let ranges = extract_fold_ranges(&rope);
        assert_eq!(ranges.len(), 1, "expected one outer (…) fold, got {ranges:?}");
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 4);
    }

    #[test]
    fn brackets_inside_strings_are_ignored() {
        let sql = "SELECT '(' , '{' , '[' FROM t WHERE x IN (\n  1,\n  2\n);\n";
        let rope = Rope::from(sql);
        let ranges = extract_fold_ranges(&rope);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 3);
    }

    #[test]
    fn multiline_block_comment_is_foldable() {
        let src = "/*\n line a\n line b\n*/\nSELECT 1;\n";
        let rope = Rope::from(src);
        let ranges = extract_fold_ranges(&rope);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 3);
    }

    #[test]
    fn outer_fold_wins_on_shared_start_line() {
        let src = "( {\n 1,\n 2\n},\n)\n";
        let rope = Rope::from(src);
        let ranges = extract_fold_ranges(&rope);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 4);
    }
}
