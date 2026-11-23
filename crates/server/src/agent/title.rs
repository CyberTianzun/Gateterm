//! 纯标题规范化、噪声过滤与东亚显示宽度智能截断。

use super::AgentAdapter;

pub const SESSION_NAME_MAX_COLS: usize = 36;
pub const FULL_TITLE_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TitleRank {
    FirstPrompt = 1,
    OscTitle = 2,
    ExplicitSummary = 3,
}

const SHELL_BINARIES: &[&str] = &[
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "cmd",
    "cmd.exe",
    "bash",
    "zsh",
    "sh",
    "fish",
    "nu",
    "wsl",
    "wsl.exe",
    "windowspowershell",
    "windows powershell",
    "administrator: windows powershell",
];

const GENERIC_STATUS: &[&str] = &[
    "thinking",
    "working",
    "loading",
    "ready",
    "idle",
    "new session",
    "new chat",
    "untitled",
    "gateterm",
    "terminal",
];

pub fn clean_title(raw: &str, rank: TitleRank, adapter: &dyn AgentAdapter) -> Option<String> {
    let stripped = strip_ansi_and_controls(raw);
    let source = if rank == TitleRank::FirstPrompt {
        super::history::prompt_body(&stripped)?
    } else {
        stripped.as_str()
    };

    let line = first_meaningful_line(source);
    let collapsed = collapse_whitespace(line);
    if collapsed.is_empty() {
        return None;
    }

    let prefixes = collect_brand_prefixes(adapter);
    let mut cur = collapsed;
    for _ in 0..4 {
        let step1 = strip_leading_spinners(&cur);
        let step2 = strip_markdown_and_quotes(step1);
        let step3 = strip_brand_prefix(&step2, &prefixes);
        let next = collapse_whitespace(step3);
        if next == cur {
            break;
        }
        cur = next;
    }

    if !cur.chars().any(char::is_alphanumeric) {
        return None;
    }
    if prefixes.iter().any(|p| eq_ci(&cur, p)) {
        return None;
    }
    if rank == TitleRank::OscTitle && is_osc_noise(&cur) {
        return None;
    }

    Some(cap_full_title(&cur))
}

pub fn display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

pub fn truncate_session_name(clean: &str, max_cols: usize) -> String {
    if display_width(clean) <= max_cols {
        return clean.to_string();
    }
    if max_cols == 0 {
        return String::new();
    }

    let limit = max_cols.saturating_sub(1);
    let mut cols = 0usize;
    let mut exact_end = 0usize;
    let mut clause_cut: Option<usize> = None;
    let mut word_cut: Option<usize> = None;

    let mut iter = clean.char_indices().peekable();
    while let Some((idx, ch)) = iter.next() {
        let w = char_display_width(ch);
        if cols + w > limit {
            break;
        }
        cols += w;
        let end = idx + ch.len_utf8();
        exact_end = end;

        let next_ch = iter.peek().map(|&(_, c)| c);
        let is_cjk_or_semi_clause = matches!(ch, '。' | '！' | '？' | ';' | '；' | '，');
        let is_ascii_clause =
            matches!(ch, '.' | '!' | '?' | ',') && next_ch.is_some_and(|n| n.is_ascii_whitespace());
        if (is_cjk_or_semi_clause || is_ascii_clause) && cols >= limit / 2 {
            clause_cut = Some(end);
        }
        if ch.is_ascii_whitespace() && cols >= limit * 2 / 3 {
            word_cut = Some(idx);
        }
    }

    let cut = clause_cut.or(word_cut).unwrap_or(exact_end);
    let slice = clean[..cut].trim_end_matches(|c: char| {
        c.is_whitespace()
            || c == '\u{200D}'
            || matches!(c, ',' | '，' | ';' | '；' | ':' | '：' | '-' | '—' | '/' | '(')
    });
    let prefix = if slice.is_empty() {
        clean[..exact_end].trim_end_matches(|c: char| c.is_whitespace() || c == '\u{200D}')
    } else {
        slice
    };
    let mut out = String::with_capacity(prefix.len() + '…'.len_utf8());
    out.push_str(prefix);
    out.push('…');
    out
}

fn char_display_width(c: char) -> usize {
    if c.is_control()
        || matches!(
            c,
            '\u{0300}'..='\u{036F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
        )
    {
        return 0;
    }
    if matches!(
        c,
        '\u{1100}'..='\u{115F}'
            | '\u{2E80}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7A3}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE10}'..='\u{FE6F}'
            | '\u{FF01}'..='\u{FF60}'
            | '\u{FFE0}'..='\u{FFE6}'
            | '\u{1F300}'..='\u{1FAFF}'
            | '\u{20000}'..='\u{3FFFF}'
    ) {
        return 2;
    }
    1
}

fn strip_ansi_and_controls(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for ch in chars.by_ref() {
                        if ('@'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' || ch == '\u{9c}' {
                            break;
                        }
                        if ch == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(ch) if (' '..='/').contains(&ch) => {
                    chars.next();
                    for ch in chars.by_ref() {
                        if ('0'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                Some(ch) if ('0'..='~').contains(&ch) => {
                    chars.next();
                }
                _ => {}
            }
            continue;
        }
        if c == '\u{9b}' {
            for ch in chars.by_ref() {
                if ('@'..='~').contains(&ch) {
                    break;
                }
            }
            continue;
        }
        if c == '\u{9d}' {
            while let Some(ch) = chars.next() {
                if ch == '\x07' || ch == '\u{9c}' {
                    break;
                }
                if ch == '\x1b' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
            continue;
        }
        if (c <= '\x1f' && !matches!(c, '\r' | '\n' | '\t'))
            || c == '\x7f'
            || ('\u{80}'..='\u{9f}').contains(&c)
        {
            continue;
        }
        out.push(c);
    }
    out
}

fn is_fence_delimiter(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix("```") {
        return !rest.contains("```");
    }
    if let Some(rest) = trimmed.strip_prefix("~~~") {
        return !rest.contains("~~~");
    }
    false
}

fn first_meaningful_line(s: &str) -> &str {
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() || is_fence_delimiter(t) {
            continue;
        }
        return t;
    }
    ""
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_spinner_glyph(c: char) -> bool {
    ('\u{2800}'..='\u{28FF}').contains(&c)
        || matches!(
            c,
            '✳' | '✽'
                | '✻'
                | '✶'
                | '✢'
                | '·'
                | '•'
                | '●'
                | '○'
                | '◐'
                | '◓'
                | '◑'
                | '◒'
                | '⏳'
                | '⌛'
                | '⚡'
                | '🤖'
                | '✨'
                | '▶'
                | '▸'
                | '›'
                | '>'
        )
}

fn is_separator(c: char) -> bool {
    matches!(c, ':' | '：' | '-' | '–' | '—' | '|' | '·' | '/')
}

fn strip_leading_spinners(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        let Some(c) = s.chars().next() else {
            return s;
        };
        if !is_spinner_glyph(c) {
            return s;
        }
        let mut after = &s[c.len_utf8()..];
        while let Some(vs) = after.chars().next() {
            if matches!(vs, '\u{FE0E}' | '\u{FE0F}') {
                after = &after[vs.len_utf8()..];
            } else {
                break;
            }
        }
        let next = after.chars().next();
        if c == '>'
            && let Some(n) = next
            && !n.is_whitespace()
            && !is_separator(n)
            && n != '>'
        {
            return s;
        }
        s = after.trim_start();
        if let Some(sep) = s.chars().next()
            && is_separator(sep)
        {
            let after_sep = &s[sep.len_utf8()..];
            if after_sep.is_empty() || after_sep.chars().next().is_some_and(char::is_whitespace) {
                s = after_sep.trim_start();
            }
        }
    }
}

fn strip_markdown_and_quotes(s: &str) -> String {
    let mut cur = s.trim();
    let hashes = cur.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes)
        && cur[hashes..].chars().next().is_some_and(char::is_whitespace)
    {
        cur = cur[hashes..].trim_start();
    }
    while let Some(first) = cur.chars().next() {
        if matches!(first, '-' | '*') && cur[1..].chars().next().is_some_and(char::is_whitespace)
        {
            cur = cur[1..].trim_start();
        } else {
            break;
        }
    }
    loop {
        cur = cur.trim();
        let mut stripped = None;
        for q in ['"', '\'', '`'] {
            if cur.len() >= 2
                && let Some(inner) = cur.strip_prefix(q).and_then(|r| r.strip_suffix(q))
                && !inner.contains(q)
            {
                stripped = Some(inner);
                break;
            }
        }
        match stripped {
            Some(inner) => cur = inner,
            None => break,
        }
    }
    unwrap_inline_backticks(cur)
}

fn unwrap_inline_backticks(s: &str) -> String {
    if !s.contains('`') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('`')
            && close > 0
        {
            out.push_str(&after_open[..close]);
            rest = &after_open[close + 1..];
        } else {
            out.push('`');
            rest = after_open;
        }
    }
    out.push_str(rest);
    out
}

fn eq_ci(a: &str, b: &str) -> bool {
    let mut ac = a.chars();
    let mut bc = b.chars();
    loop {
        match (ac.next(), bc.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x.to_lowercase().eq(y.to_lowercase()) => {}
            _ => return false,
        }
    }
}

fn strip_ci_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut s_chars = s.char_indices();
    for p_ch in prefix.chars() {
        let (_, s_ch) = s_chars.next()?;
        if !s_ch.to_lowercase().eq(p_ch.to_lowercase()) {
            return None;
        }
    }
    let byte_end = s_chars.next().map_or(s.len(), |(idx, _)| idx);
    Some(&s[byte_end..])
}

fn collect_brand_prefixes<'a>(adapter: &'a dyn AgentAdapter) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = Vec::new();
    let mut push_unique = |s: &'a str| {
        let t = s.trim();
        if !t.is_empty() && !out.iter().any(|existing| eq_ci(existing, t)) {
            out.push(t);
        }
    };
    push_unique(adapter.name());
    for &p in adapter.title_prefixes() {
        push_unique(p);
    }
    for &a in super::registry() {
        push_unique(a.name());
        for &p in a.title_prefixes() {
            push_unique(p);
        }
    }
    out.sort_by(|a, b| {
        b.chars()
            .count()
            .cmp(&a.chars().count())
            .then_with(|| b.len().cmp(&a.len()))
    });
    out
}

fn strip_brand_prefix<'a>(s: &'a str, prefixes: &[&str]) -> &'a str {
    let trimmed = s.trim_start();
    for &prefix in prefixes {
        let Some(after_prefix) = strip_ci_prefix(trimmed, prefix) else {
            continue;
        };
        let after_ws = after_prefix.trim_start();
        if let Some(sep) = after_ws.chars().next()
            && is_separator(sep)
        {
            return after_ws[sep.len_utf8()..].trim_start();
        }
    }
    trimmed
}

fn is_windows_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || matches!(b[2], b'\\' | b'/'))
}

fn is_bare_fs_path(s: &str) -> bool {
    s == "~"
        || s.starts_with("~/")
        || s.starts_with("~\\")
        || s.starts_with('/')
        || s.starts_with("\\\\")
        || is_windows_drive_path(s)
}

fn is_user_at_host_prompt(s: &str) -> bool {
    let Some((lhs, rhs)) = s.split_once(':') else {
        return false;
    };
    if lhs.is_empty() || lhs.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((user, host)) = lhs.split_once('@') else {
        return false;
    };
    if user.is_empty() || host.is_empty() {
        return false;
    }
    let rhs_t = rhs.trim();
    is_bare_fs_path(rhs_t)
        || rhs_t == "."
        || rhs_t.starts_with("./")
        || rhs_t.starts_with(".\\")
        || rhs_t.starts_with("../")
        || rhs_t.starts_with("..\\")
}

fn is_osc_noise(s: &str) -> bool {
    if is_bare_fs_path(s) || is_user_at_host_prompt(s) {
        return true;
    }
    if SHELL_BINARIES.iter().any(|b| eq_ci(s, b)) {
        return true;
    }
    if (!s.contains(char::is_whitespace) || is_bare_fs_path(s))
        && let Some(base) = s.rsplit(['/', '\\']).next()
    {
        let base_trimmed = base.trim_start_matches('-');
        if SHELL_BINARIES
            .iter()
            .any(|b| eq_ci(base, b) || eq_ci(base_trimmed, b))
        {
            return true;
        }
    }
    let status_norm = s.trim_end_matches(['.', '…', ' ']);
    GENERIC_STATUS.iter().any(|g| eq_ci(status_norm, g))
}

fn cap_full_title(s: &str) -> String {
    if s.chars().count() <= FULL_TITLE_MAX_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(FULL_TITLE_MAX_CHARS - 1).collect();
    while out.ends_with(char::is_whitespace) {
        out.pop();
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::find;

    #[test]
    fn strips_spinners_markdown_ansi_and_brand_prefixes() {
        let claude = find("claude-code").unwrap();
        let oc = find("opencode").unwrap();
        let pi = find("pi").unwrap();
        let copilot = find("copilot").unwrap();
        let agy = find("antigravity").unwrap();

        assert_eq!(
            clean_title("✳ Claude Code: Fix clipboard sync", TitleRank::OscTitle, claude)
                .as_deref(),
            Some("Fix clipboard sync")
        );
        assert_eq!(
            clean_title(
                "\x1b[32m✳ Claude Code:\x1b[0m Fix clipboard sync",
                TitleRank::OscTitle,
                claude,
            )
            .as_deref(),
            Some("Fix clipboard sync")
        );
        assert_eq!(
            clean_title(
                "Claude Code · ⠋ Fix clipboard sync",
                TitleRank::OscTitle,
                claude,
            )
            .as_deref(),
            Some("Fix clipboard sync")
        );
        assert_eq!(
            clean_title("⠋ OC | Refactor auth", TitleRank::OscTitle, oc).as_deref(),
            Some("Refactor auth")
        );
        assert_eq!(
            clean_title("π - Add unit tests", TitleRank::OscTitle, pi).as_deref(),
            Some("Add unit tests")
        );
        assert_eq!(
            clean_title("GitHub Copilot: Review PR", TitleRank::OscTitle, copilot).as_deref(),
            Some("Review PR")
        );
        assert_eq!(
            clean_title("AGY / Fix bug", TitleRank::OscTitle, agy).as_deref(),
            Some("Fix bug")
        );
        assert_eq!(
            clean_title("## `Fix` login\nsecond line", TitleRank::FirstPrompt, claude)
                .as_deref(),
            Some("Fix login")
        );
        assert_eq!(
            clean_title("```rust\nfn fix_login() {}\n```", TitleRank::FirstPrompt, claude)
                .as_deref(),
            Some("fn fix_login() {}")
        );
        assert_eq!(
            clean_title(
                "\n~~~\nFirst meaningful line\n~~~",
                TitleRank::FirstPrompt,
                claude,
            )
            .as_deref(),
            Some("First meaningful line")
        );
        assert_eq!(
            clean_title(
                "Claude is helping with pipeline",
                TitleRank::FirstPrompt,
                claude,
            )
            .as_deref(),
            Some("Claude is helping with pipeline")
        );
        assert_eq!(
            clean_title("\"Fix 'auth' bug\"", TitleRank::FirstPrompt, claude).as_deref(),
            Some("Fix 'auth' bug")
        );
        assert_eq!(
            clean_title("\"a\" and \"b\"", TitleRank::FirstPrompt, claude).as_deref(),
            Some("\"a\" and \"b\"")
        );
    }

    #[test]
    fn rejects_noise_bare_brands_shells_and_paths_for_osc() {
        let claude = find("claude-code").unwrap();
        let codex = find("codex").unwrap();

        for bad in [
            "Claude Code",
            "Claude",
            "✳",
            "powershell.exe",
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "Administrator: Windows PowerShell",
            "bash",
            "/usr/bin/zsh",
            "user@host: ~/project",
            "C:\\Users\\foo",
            "~/work",
            "/tmp/dir",
            "thinking",
            "Working",
            "loading...",
            "...",
            "OpenCode",
            "π",
            "pi",
            "AGY",
            "Copilot",
        ] {
            assert_eq!(
                clean_title(bad, TitleRank::OscTitle, claude),
                None,
                "expected OscTitle {bad:?} to be rejected"
            );
        }

        assert_eq!(
            clean_title("fix /bin/bash symlink", TitleRank::OscTitle, claude).as_deref(),
            Some("fix /bin/bash symlink")
        );
        assert_eq!(
            clean_title("email user@foo.com: fix login", TitleRank::OscTitle, claude).as_deref(),
            Some("email user@foo.com: fix login")
        );

        assert_eq!(
            clean_title(
                "<environment_context>host</environment_context>",
                TitleRank::FirstPrompt,
                codex,
            ),
            None
        );
        assert_eq!(
            clean_title(
                "# AGENTS.md instructions\nfollow rules",
                TitleRank::FirstPrompt,
                codex,
            ),
            None
        );
        assert_eq!(
            clean_title(
                "<system-reminder>ignore</system-reminder>",
                TitleRank::FirstPrompt,
                codex,
            ),
            None
        );
        assert_eq!(
            clean_title(
                "## My request for Codex:\nFix clipboard\nMore details",
                TitleRank::FirstPrompt,
                codex,
            )
            .as_deref(),
            Some("Fix clipboard")
        );
    }

    #[test]
    fn smart_truncates_cjk_english_emoji_and_combining_marks() {
        assert_eq!(
            truncate_session_name("Fix clipboard sync", SESSION_NAME_MAX_COLS),
            "Fix clipboard sync"
        );
        let exact36 = "a".repeat(36);
        assert_eq!(truncate_session_name(&exact36, SESSION_NAME_MAX_COLS), exact36);

        let eng = truncate_session_name(
            "Implement multi-source session auto-naming across all seven adapters",
            SESSION_NAME_MAX_COLS,
        );
        assert!(eng.ends_with('…'), "{eng:?}");
        assert!(display_width(&eng) <= SESSION_NAME_MAX_COLS, "{eng:?}");
        assert_eq!(eng, "Implement multi-source session…");

        let zh = truncate_session_name(
            "实现服务端多源会话自动命名与长标题智能截断优化以及悬停完整提示",
            SESSION_NAME_MAX_COLS,
        );
        assert!(zh.ends_with('…'), "{zh:?}");
        assert!(display_width(&zh) <= SESSION_NAME_MAX_COLS, "{zh:?}");

        let dangling = truncate_session_name(
            "实现服务端多源会话自动命名与长标：详细说明补充测试内容其他",
            SESSION_NAME_MAX_COLS,
        );
        assert!(dangling.ends_with('…'), "{dangling:?}");
        assert!(!dangling.ends_with("：…"), "{dangling:?}");
        assert!(display_width(&dangling) <= SESSION_NAME_MAX_COLS, "{dangling:?}");

        assert_eq!(
            truncate_session_name(
                "修复 Web 终端尺寸问题，顺便补充重连测试和其他细节说明",
                SESSION_NAME_MAX_COLS,
            ),
            "修复 Web 终端尺寸问题…"
        );

        assert_eq!(display_width("cafe\u{0301}"), 4);
        assert_eq!(display_width("🚀"), 2);
        assert_eq!(display_width("中文"), 4);

        let emoji_input = format!("🚀 café\u{0301} {}", "测试".repeat(20));
        let emoji_out = truncate_session_name(&emoji_input, SESSION_NAME_MAX_COLS);
        assert!(emoji_out.ends_with('…'), "{emoji_out:?}");
        assert!(display_width(&emoji_out) <= SESSION_NAME_MAX_COLS, "{emoji_out:?}");

        let codex = find("codex").unwrap();
        let long = "x".repeat(250);
        let capped = clean_title(&long, TitleRank::FirstPrompt, codex).unwrap();
        assert_eq!(capped.chars().count(), FULL_TITLE_MAX_CHARS);
        assert!(capped.ends_with('…'));

        let exact200 = "y".repeat(200);
        assert_eq!(
            clean_title(&exact200, TitleRank::FirstPrompt, codex).as_deref(),
            Some(exact200.as_str())
        );
    }
}
