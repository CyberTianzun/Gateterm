use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Flat arena-indexed Markdown AST produced by [`parse_markdown`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdDocument {
    pub blocks: Vec<MdBlock>,
    pub root: Vec<u32>,
}

/// A block-level Markdown node stored inside [`MdDocument::blocks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdBlock {
    Heading {
        level: u8,
        runs: Vec<MdRun>,
    },
    Paragraph {
        runs: Vec<MdRun>,
    },
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    BlockQuote {
        children: Vec<u32>,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<MdListItem>,
    },
    Table {
        alignments: Vec<MdAlign>,
        headers: Vec<MdTableCell>,
        rows: Vec<Vec<MdTableCell>>,
    },
    ThematicBreak,
}

/// An item inside [`MdBlock::List`], optionally carrying a GFM task-list checkbox state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdListItem {
    pub checked: Option<bool>,
    pub children: Vec<u32>,
}

/// A single cell in a GFM [`MdBlock::Table`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdTableCell {
    pub runs: Vec<MdRun>,
}

/// Column text alignment for a GFM [`MdBlock::Table`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdAlign {
    None,
    Left,
    Center,
    Right,
}

/// A styled inline text span with optional sanitized hyperlink target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// Validates a link destination against the allowed URI scheme whitelist
/// (`http://`, `https://`, `mailto:`, case-insensitive) and rejects empty
/// strings or strings containing ASCII control characters.
pub fn sanitize_link_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.bytes().any(|b| b <= 0x1f || b == 0x7f) {
        return None;
    }
    let scheme_len = ["https://", "http://", "mailto:"]
        .into_iter()
        .find(|scheme| {
            trimmed
                .get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
        .map(str::len)?;

    if trimmed.len() <= scheme_len {
        return None;
    }
    Some(trimmed.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
}

struct PendingImage {
    alt: String,
    link: Option<String>,
}

struct PendingText {
    buf: String,
    style: InlineStyle,
}

enum Container {
    Root {
        children: Vec<u32>,
        pending_runs: Vec<MdRun>,
    },
    BlockQuote {
        children: Vec<u32>,
        pending_runs: Vec<MdRun>,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<MdListItem>,
    },
    ListItem {
        checked: Option<bool>,
        children: Vec<u32>,
        pending_runs: Vec<MdRun>,
    },
    Heading {
        level: u8,
        runs: Vec<MdRun>,
    },
    Paragraph {
        runs: Vec<MdRun>,
    },
    HtmlBlock {
        text: String,
    },
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    Table {
        alignments: Vec<MdAlign>,
        headers: Vec<MdTableCell>,
        rows: Vec<Vec<MdTableCell>>,
    },
    TableHead {
        cells: Vec<MdTableCell>,
    },
    TableRow {
        cells: Vec<MdTableCell>,
    },
    TableCell {
        runs: Vec<MdRun>,
    },
}

struct ParseState {
    blocks: Vec<MdBlock>,
    stack: Vec<Container>,
    bold_depth: usize,
    italic_depth: usize,
    strike_depth: usize,
    link_stack: Vec<Option<String>>,
    image_stack: Vec<PendingImage>,
    pending_text: Option<PendingText>,
}

impl ParseState {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            stack: vec![Container::Root {
                children: Vec::new(),
                pending_runs: Vec::new(),
            }],
            bold_depth: 0,
            italic_depth: 0,
            strike_depth: 0,
            link_stack: Vec::new(),
            image_stack: Vec::new(),
            pending_text: None,
        }
    }

    fn current_style(&self) -> InlineStyle {
        InlineStyle {
            bold: self.bold_depth > 0,
            italic: self.italic_depth > 0,
            strikethrough: self.strike_depth > 0,
        }
    }

    fn current_link(&self) -> Option<String> {
        self.link_stack.last().and_then(Clone::clone)
    }

    fn push_styled_run(&mut self, text: String, code: bool) {
        let style = self.current_style();
        let link = self.current_link();
        push_run_to_top(
            &mut self.stack,
            MdRun {
                text,
                bold: style.bold,
                italic: style.italic,
                strikethrough: style.strikethrough,
                code,
                link,
            },
        );
    }

    fn flush_pending_text(&mut self) {
        if let Some(pt) = self.pending_text.take() {
            push_text_with_bare_urls(&mut self.stack, &pt.buf, pt.style);
        }
    }

    fn flush_pending_runs(&mut self) {
        flush_pending_runs(&mut self.stack, &mut self.blocks);
    }

    fn attach_block(&mut self, block: MdBlock) {
        attach_block(&mut self.stack, &mut self.blocks, block);
    }

    fn handle_event(&mut self, event: Event<'_>) {
        // Coalesce consecutive Event::Text nodes with identical style so bare URLs
        // split by pulldown-cmark at `_` or entities stay intact before scanning.
        if !matches!(event, Event::Text(_)) {
            self.flush_pending_text();
        }
        match event {
            Event::Start(tag) => self.handle_start(tag),
            Event::End(tag_end) => self.handle_end(tag_end),
            Event::Text(text) => self.handle_text(text.into_string()),
            Event::Code(code_text) => {
                if let Some(img) = self.image_stack.last_mut() {
                    img.alt.push_str(code_text.as_ref());
                } else {
                    self.push_styled_run(code_text.into_string(), true);
                }
            }
            Event::Html(html) => {
                if let Some(img) = self.image_stack.last_mut() {
                    img.alt.push_str(html.as_ref());
                } else if let Some(Container::HtmlBlock { text }) = self.stack.last_mut() {
                    text.push_str(html.as_ref());
                } else {
                    self.push_styled_run(html.into_string(), false);
                }
            }
            Event::InlineHtml(html) => {
                if let Some(img) = self.image_stack.last_mut() {
                    img.alt.push_str(html.as_ref());
                } else {
                    self.push_styled_run(html.into_string(), false);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(img) = self.image_stack.last_mut() {
                    img.alt.push(' ');
                } else if let Some(Container::CodeBlock { code, .. }) = self.stack.last_mut() {
                    code.push('\n');
                } else {
                    self.push_styled_run("\n".to_string(), false);
                }
            }
            Event::Rule => {
                self.flush_pending_runs();
                self.attach_block(MdBlock::ThematicBreak);
            }
            Event::TaskListMarker(checked) => {
                for container in self.stack.iter_mut().rev() {
                    if let Container::ListItem {
                        checked: item_checked,
                        ..
                    } = container
                    {
                        *item_checked = Some(checked);
                        break;
                    }
                }
            }
            Event::InlineMath(s) | Event::DisplayMath(s) | Event::FootnoteReference(s) => {
                if let Some(img) = self.image_stack.last_mut() {
                    img.alt.push_str(s.as_ref());
                } else {
                    self.push_styled_run(s.into_string(), false);
                }
            }
        }
    }

    fn handle_text(&mut self, text: String) {
        if !self.image_stack.is_empty() {
            self.flush_pending_text();
            if let Some(img) = self.image_stack.last_mut() {
                img.alt.push_str(&text);
            }
            return;
        }
        if matches!(self.stack.last(), Some(Container::CodeBlock { .. })) {
            self.flush_pending_text();
            if let Some(Container::CodeBlock { code, .. }) = self.stack.last_mut() {
                code.push_str(&text);
            }
            return;
        }
        if !self.link_stack.is_empty() {
            self.flush_pending_text();
            self.push_styled_run(text, false);
            return;
        }
        let style = self.current_style();
        match &mut self.pending_text {
            Some(pt) if pt.style == style => pt.buf.push_str(&text),
            _ => {
                self.flush_pending_text();
                self.pending_text = Some(PendingText { buf: text, style });
            }
        }
    }

    fn handle_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.flush_pending_runs();
                self.stack.push(Container::Paragraph { runs: Vec::new() });
            }
            Tag::Heading { level, .. } => {
                self.flush_pending_runs();
                self.stack.push(Container::Heading {
                    level: heading_level_to_u8(level),
                    runs: Vec::new(),
                });
            }
            Tag::BlockQuote(_) => {
                self.flush_pending_runs();
                self.stack.push(Container::BlockQuote {
                    children: Vec::new(),
                    pending_runs: Vec::new(),
                });
            }
            Tag::CodeBlock(kind) => {
                self.flush_pending_runs();
                let lang = match kind {
                    CodeBlockKind::Indented => None,
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                };
                self.stack.push(Container::CodeBlock {
                    lang,
                    code: String::new(),
                });
            }
            Tag::HtmlBlock => {
                self.flush_pending_runs();
                self.stack.push(Container::HtmlBlock {
                    text: String::new(),
                });
            }
            Tag::List(start) => {
                self.flush_pending_runs();
                self.stack.push(Container::List {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.stack.push(Container::ListItem {
                    checked: None,
                    children: Vec::new(),
                    pending_runs: Vec::new(),
                });
            }
            Tag::Table(alignments) => {
                self.flush_pending_runs();
                self.stack.push(Container::Table {
                    alignments: alignments.into_iter().map(map_alignment).collect(),
                    headers: Vec::new(),
                    rows: Vec::new(),
                });
            }
            Tag::TableHead => {
                self.stack.push(Container::TableHead { cells: Vec::new() });
            }
            Tag::TableRow => {
                self.stack.push(Container::TableRow { cells: Vec::new() });
            }
            Tag::TableCell => {
                self.stack.push(Container::TableCell { runs: Vec::new() });
            }
            Tag::Emphasis => self.italic_depth += 1,
            Tag::Strong => self.bold_depth += 1,
            Tag::Strikethrough => self.strike_depth += 1,
            Tag::Link { dest_url, .. } => {
                self.link_stack.push(sanitize_link_url(dest_url.as_ref()));
            }
            Tag::Image { dest_url, .. } => {
                self.image_stack.push(PendingImage {
                    alt: String::new(),
                    link: sanitize_link_url(dest_url.as_ref()),
                });
            }
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn handle_end(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Paragraph => {
                if let Some(Container::Paragraph { runs }) = self.stack.pop() {
                    self.attach_block(MdBlock::Paragraph { runs });
                }
            }
            TagEnd::Heading(_) => {
                if let Some(Container::Heading { level, runs }) = self.stack.pop() {
                    self.attach_block(MdBlock::Heading { level, runs });
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Container::BlockQuote {
                    mut children,
                    mut pending_runs,
                }) = self.stack.pop()
                {
                    push_pending_paragraph(&mut self.blocks, &mut children, &mut pending_runs);
                    self.attach_block(MdBlock::BlockQuote { children });
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Container::CodeBlock { lang, mut code }) = self.stack.pop() {
                    strip_one_trailing_newline(&mut code);
                    self.attach_block(MdBlock::CodeBlock { lang, code });
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(Container::HtmlBlock { mut text }) = self.stack.pop() {
                    strip_one_trailing_newline(&mut text);
                    if !text.is_empty() {
                        self.attach_block(MdBlock::Paragraph {
                            runs: vec![MdRun {
                                text,
                                bold: false,
                                italic: false,
                                strikethrough: false,
                                code: false,
                                link: None,
                            }],
                        });
                    }
                }
            }
            TagEnd::List(_) => {
                if let Some(Container::List {
                    ordered,
                    start,
                    items,
                }) = self.stack.pop()
                {
                    self.attach_block(MdBlock::List {
                        ordered,
                        start,
                        items,
                    });
                }
            }
            TagEnd::Item => {
                if let Some(Container::ListItem {
                    checked,
                    mut children,
                    mut pending_runs,
                }) = self.stack.pop()
                {
                    push_pending_paragraph(&mut self.blocks, &mut children, &mut pending_runs);
                    if let Some(Container::List { items, .. }) = self.stack.last_mut() {
                        items.push(MdListItem { checked, children });
                    }
                }
            }
            TagEnd::Table => {
                if let Some(Container::Table {
                    alignments,
                    headers,
                    rows,
                }) = self.stack.pop()
                {
                    self.attach_block(MdBlock::Table {
                        alignments,
                        headers,
                        rows,
                    });
                }
            }
            TagEnd::TableHead => {
                if let Some(Container::TableHead { cells }) = self.stack.pop()
                    && let Some(Container::Table { headers, .. }) = self.stack.last_mut()
                {
                    *headers = cells;
                }
            }
            TagEnd::TableRow => {
                if let Some(Container::TableRow { cells }) = self.stack.pop()
                    && let Some(Container::Table { rows, .. }) = self.stack.last_mut()
                {
                    rows.push(cells);
                }
            }
            TagEnd::TableCell => {
                if let Some(Container::TableCell { runs }) = self.stack.pop() {
                    match self.stack.last_mut() {
                        Some(Container::TableHead { cells })
                        | Some(Container::TableRow { cells }) => {
                            cells.push(MdTableCell { runs });
                        }
                        _ => {}
                    }
                }
            }
            TagEnd::Emphasis => self.italic_depth = self.italic_depth.saturating_sub(1),
            TagEnd::Strong => self.bold_depth = self.bold_depth.saturating_sub(1),
            TagEnd::Strikethrough => self.strike_depth = self.strike_depth.saturating_sub(1),
            TagEnd::Link => {
                self.link_stack.pop();
            }
            TagEnd::Image => {
                if let Some(img) = self.image_stack.pop() {
                    let rendered_alt = if img.alt.is_empty() {
                        format!("[{}]", img.link.as_deref().unwrap_or("image"))
                    } else {
                        format!("[{}]", img.alt)
                    };
                    if let Some(outer) = self.image_stack.last_mut() {
                        outer.alt.push_str(&rendered_alt);
                    } else {
                        let effective_link = match self.link_stack.last() {
                            Some(outer) => outer.clone(),
                            None => img.link,
                        };
                        let style = self.current_style();
                        push_run_to_top(
                            &mut self.stack,
                            MdRun {
                                text: rendered_alt,
                                bold: style.bold,
                                italic: style.italic,
                                strikethrough: style.strikethrough,
                                code: false,
                                link: effective_link,
                            },
                        );
                    }
                }
            }
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn finish(mut self) -> MdDocument {
        self.flush_pending_text();
        self.flush_pending_runs();
        let root = match self.stack.pop() {
            Some(Container::Root { children, .. }) => children,
            _ => Vec::new(),
        };
        MdDocument {
            blocks: self.blocks,
            root,
        }
    }
}

/// Parses CommonMark + GFM (tables, strikethrough, task lists) into a flat
/// arena [`MdDocument`] with sanitized links, literal raw HTML, and bare URL autolinking.
pub fn parse_markdown(input: &str) -> MdDocument {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut state = ParseState::new();
    for event in Parser::new_ext(input, options) {
        state.handle_event(event);
    }
    state.finish()
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn map_alignment(align: Alignment) -> MdAlign {
    match align {
        Alignment::None => MdAlign::None,
        Alignment::Left => MdAlign::Left,
        Alignment::Center => MdAlign::Center,
        Alignment::Right => MdAlign::Right,
    }
}

fn strip_one_trailing_newline(s: &mut String) {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
}

fn alloc_block(blocks: &mut Vec<MdBlock>, block: MdBlock) -> u32 {
    let idx = u32::try_from(blocks.len()).expect("markdown block count exceeds u32::MAX");
    blocks.push(block);
    idx
}

fn push_pending_paragraph(
    blocks: &mut Vec<MdBlock>,
    children: &mut Vec<u32>,
    pending_runs: &mut Vec<MdRun>,
) {
    if !pending_runs.is_empty() {
        let runs = std::mem::take(pending_runs);
        let idx = alloc_block(blocks, MdBlock::Paragraph { runs });
        children.push(idx);
    }
}

fn flush_pending_runs(stack: &mut [Container], blocks: &mut Vec<MdBlock>) {
    let Some(top) = stack.last_mut() else {
        return;
    };
    let (children, pending_runs) = match top {
        Container::Root {
            children,
            pending_runs,
        }
        | Container::BlockQuote {
            children,
            pending_runs,
        }
        | Container::ListItem {
            children,
            pending_runs,
            ..
        } => (children, pending_runs),
        _ => return,
    };

    push_pending_paragraph(blocks, children, pending_runs);
}

fn attach_block(stack: &mut [Container], blocks: &mut Vec<MdBlock>, block: MdBlock) {
    let idx = alloc_block(blocks, block);
    if let Some(
        Container::Root { children, .. }
        | Container::BlockQuote { children, .. }
        | Container::ListItem { children, .. },
    ) = stack.last_mut()
    {
        children.push(idx);
    }
}

fn push_merged_run(runs: &mut Vec<MdRun>, run: MdRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.bold == run.bold
        && last.italic == run.italic
        && last.strikethrough == run.strikethrough
        && last.code == run.code
        && last.link == run.link
    {
        last.text.push_str(&run.text);
        return;
    }
    runs.push(run);
}

fn push_run_to_top(stack: &mut [Container], run: MdRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(
        Container::Heading { runs, .. }
        | Container::Paragraph { runs }
        | Container::TableCell { runs }
        | Container::Root {
            pending_runs: runs, ..
        }
        | Container::BlockQuote {
            pending_runs: runs, ..
        }
        | Container::ListItem {
            pending_runs: runs, ..
        },
    ) = stack.last_mut()
    {
        push_merged_run(runs, run);
    }
}

/// Splits standalone `http://` and `https://` URLs out of plain text runs while
/// checking the left boundary (so `foohttps://` is not split) and trimming
/// trailing punctuation and unbalanced closing brackets (`.,;:!?)]}`).
fn push_text_with_bare_urls(stack: &mut [Container], text: &str, style: InlineStyle) {
    let mut cursor = 0;
    let mut idx = 0;

    while idx < text.len() {
        let rest = &text[idx..];
        let scheme_len = if starts_with_ignore_ascii_case(rest, "https://") {
            Some(8)
        } else if starts_with_ignore_ascii_case(rest, "http://") {
            Some(7)
        } else {
            None
        };

        if let Some(scheme_len) = scheme_len {
            let prev_char = text[..idx].chars().next_back();
            let left_ok = prev_char
                .is_none_or(|c| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '.' | '/' | '\\'));
            if left_ok {
                let raw_end = idx + find_url_candidate_end(rest);
                let trimmed_len = trim_url_end(&text[idx..raw_end]);
                if trimmed_len > scheme_len
                    && let Some(sanitized) = sanitize_link_url(&text[idx..idx + trimmed_len])
                {
                    if idx > cursor {
                        push_run_to_top(
                            stack,
                            MdRun {
                                text: text[cursor..idx].to_string(),
                                bold: style.bold,
                                italic: style.italic,
                                strikethrough: style.strikethrough,
                                code: false,
                                link: None,
                            },
                        );
                    }
                    let url_end = idx + trimmed_len;
                    push_run_to_top(
                        stack,
                        MdRun {
                            text: text[idx..url_end].to_string(),
                            bold: style.bold,
                            italic: style.italic,
                            strikethrough: style.strikethrough,
                            code: false,
                            link: Some(sanitized),
                        },
                    );
                    cursor = url_end;
                    idx = url_end;
                    continue;
                }
            }
        }

        let ch_len = rest.chars().next().map_or(1, char::len_utf8);
        idx += ch_len;
    }

    if cursor < text.len() {
        push_run_to_top(
            stack,
            MdRun {
                text: text[cursor..].to_string(),
                bold: style.bold,
                italic: style.italic,
                strikethrough: style.strikethrough,
                code: false,
                link: None,
            },
        );
    }
}

fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    haystack
        .get(..prefix.len())
        .is_some_and(|slice| slice.eq_ignore_ascii_case(prefix))
}

fn find_url_candidate_end(rest: &str) -> usize {
    for (byte_idx, ch) in rest.char_indices() {
        if ch.is_whitespace()
            || ch.is_control()
            || matches!(
                ch,
                '<' | '>'
                    | '"'
                    | '\''
                    | '`'
                    | '，'
                    | '。'
                    | '；'
                    | '：'
                    | '！'
                    | '？'
                    | '）'
                    | '】'
                    | '」'
                    | '、'
                    | '》'
            )
        {
            return byte_idx;
        }
    }
    rest.len()
}

fn trim_url_end(candidate: &str) -> usize {
    let mut end = candidate.len();
    loop {
        let slice = &candidate[..end];
        let Some(ch) = slice.chars().next_back() else {
            break;
        };
        let strip = match ch {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => count_char(slice, ')') > count_char(slice, '('),
            ']' => count_char(slice, ']') > count_char(slice, '['),
            '}' => count_char(slice, '}') > count_char(slice, '{'),
            _ => false,
        };
        if strip {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn count_char(s: &str, target: char) -> usize {
    s.chars().filter(|&c| c == target).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_run(text: &str) -> MdRun {
        MdRun {
            text: text.to_string(),
            bold: false,
            italic: false,
            strikethrough: false,
            code: false,
            link: None,
        }
    }

    fn concat_runs(runs: &[MdRun]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn parses_headings_paragraphs_and_soft_breaks() {
        let doc = parse_markdown("# H1\n\nline1\nline2\n\n###### H6\n\n---");
        assert_eq!(doc.root.len(), 4);
        assert_eq!(
            doc.blocks[doc.root[0] as usize],
            MdBlock::Heading {
                level: 1,
                runs: vec![plain_run("H1")],
            }
        );
        assert_eq!(
            doc.blocks[doc.root[1] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("line1\nline2")],
            }
        );
        assert_eq!(
            doc.blocks[doc.root[2] as usize],
            MdBlock::Heading {
                level: 6,
                runs: vec![plain_run("H6")],
            }
        );
        assert_eq!(doc.blocks[doc.root[3] as usize], MdBlock::ThematicBreak);
    }

    #[test]
    fn parses_inline_styles_and_merges_adjacent_runs() {
        let doc = parse_markdown("**bold *both* ~~all~~** and `code`");
        assert_eq!(doc.root.len(), 1);
        assert_eq!(
            doc.blocks[doc.root[0] as usize],
            MdBlock::Paragraph {
                runs: vec![
                    MdRun {
                        text: "bold ".to_string(),
                        bold: true,
                        italic: false,
                        strikethrough: false,
                        code: false,
                        link: None,
                    },
                    MdRun {
                        text: "both".to_string(),
                        bold: true,
                        italic: true,
                        strikethrough: false,
                        code: false,
                        link: None,
                    },
                    MdRun {
                        text: " ".to_string(),
                        bold: true,
                        italic: false,
                        strikethrough: false,
                        code: false,
                        link: None,
                    },
                    MdRun {
                        text: "all".to_string(),
                        bold: true,
                        italic: false,
                        strikethrough: true,
                        code: false,
                        link: None,
                    },
                    plain_run(" and "),
                    MdRun {
                        text: "code".to_string(),
                        bold: false,
                        italic: false,
                        strikethrough: false,
                        code: true,
                        link: None,
                    },
                ],
            }
        );

        let merged = parse_markdown("a &amp; b \\*c\\*");
        assert_eq!(
            merged.blocks[merged.root[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("a & b *c*")],
            }
        );
    }

    #[test]
    fn parses_fenced_and_unclosed_code_blocks() {
        let closed = parse_markdown("```rust\nfn a() {}\n```");
        assert_eq!(closed.root.len(), 1);
        assert_eq!(
            closed.blocks[closed.root[0] as usize],
            MdBlock::CodeBlock {
                lang: Some("rust".to_string()),
                code: "fn a() {}".to_string(),
            }
        );

        let unclosed = parse_markdown("```ts\nconst x = 1");
        assert_eq!(unclosed.root.len(), 1);
        assert_eq!(
            unclosed.blocks[unclosed.root[0] as usize],
            MdBlock::CodeBlock {
                lang: Some("ts".to_string()),
                code: "const x = 1".to_string(),
            }
        );

        let extra_info =
            parse_markdown("```\nplain\n```\n\n```python title=\"demo\"\nprint(1)\n```");
        assert_eq!(extra_info.root.len(), 2);
        assert_eq!(
            extra_info.blocks[extra_info.root[0] as usize],
            MdBlock::CodeBlock {
                lang: None,
                code: "plain".to_string(),
            }
        );
        assert_eq!(
            extra_info.blocks[extra_info.root[1] as usize],
            MdBlock::CodeBlock {
                lang: Some("python".to_string()),
                code: "print(1)".to_string(),
            }
        );
    }

    #[test]
    fn parses_nested_lists_tasklists_and_blockquotes() {
        let md = "- [x] done\n- [ ] todo\n  1. sub one\n  2. sub two\n\n> quote\n> - item\n\n3. third\n4. fourth";
        let doc = parse_markdown(md);
        assert_eq!(doc.root.len(), 3);

        let MdBlock::List {
            ordered,
            start,
            items,
        } = &doc.blocks[doc.root[0] as usize]
        else {
            panic!("expected root[0] to be List");
        };
        assert!(!ordered);
        assert_eq!(*start, 1);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[0].children.len(), 1);
        assert_eq!(
            doc.blocks[items[0].children[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("done")],
            }
        );

        assert_eq!(items[1].checked, Some(false));
        assert_eq!(items[1].children.len(), 2);
        assert_eq!(
            doc.blocks[items[1].children[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("todo")],
            }
        );
        let MdBlock::List {
            ordered: sub_ordered,
            start: sub_start,
            items: sub_items,
        } = &doc.blocks[items[1].children[1] as usize]
        else {
            panic!("expected nested ordered List");
        };
        assert!(*sub_ordered);
        assert_eq!(*sub_start, 1);
        assert_eq!(sub_items.len(), 2);
        assert_eq!(sub_items[0].checked, None);
        assert_eq!(
            doc.blocks[sub_items[0].children[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("sub one")],
            }
        );

        let MdBlock::BlockQuote { children } = &doc.blocks[doc.root[1] as usize] else {
            panic!("expected root[1] to be BlockQuote");
        };
        assert_eq!(children.len(), 2);
        assert_eq!(
            doc.blocks[children[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("quote")],
            }
        );
        let MdBlock::List {
            ordered: q_ordered,
            items: q_items,
            ..
        } = &doc.blocks[children[1] as usize]
        else {
            panic!("expected List inside BlockQuote");
        };
        assert!(!q_ordered);
        assert_eq!(q_items.len(), 1);
        assert_eq!(
            doc.blocks[q_items[0].children[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("item")],
            }
        );

        let MdBlock::List {
            ordered: third_ordered,
            start: third_start,
            items: third_items,
        } = &doc.blocks[doc.root[2] as usize]
        else {
            panic!("expected root[2] to be ordered List");
        };
        assert!(*third_ordered);
        assert_eq!(*third_start, 3);
        assert_eq!(third_items.len(), 2);
        assert_eq!(
            doc.blocks[third_items[0].children[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("third")],
            }
        );
    }

    #[test]
    fn parses_gfm_table_with_alignments_and_inline_runs() {
        let md = "| A | B | C |\n|:---|:---:|---:|\n| **1** | `2` | 3 |";
        let doc = parse_markdown(md);
        assert_eq!(doc.root.len(), 1);
        assert_eq!(
            doc.blocks[doc.root[0] as usize],
            MdBlock::Table {
                alignments: vec![MdAlign::Left, MdAlign::Center, MdAlign::Right],
                headers: vec![
                    MdTableCell {
                        runs: vec![plain_run("A")],
                    },
                    MdTableCell {
                        runs: vec![plain_run("B")],
                    },
                    MdTableCell {
                        runs: vec![plain_run("C")],
                    },
                ],
                rows: vec![vec![
                    MdTableCell {
                        runs: vec![MdRun {
                            text: "1".to_string(),
                            bold: true,
                            italic: false,
                            strikethrough: false,
                            code: false,
                            link: None,
                        }],
                    },
                    MdTableCell {
                        runs: vec![MdRun {
                            text: "2".to_string(),
                            bold: false,
                            italic: false,
                            strikethrough: false,
                            code: true,
                            link: None,
                        }],
                    },
                    MdTableCell {
                        runs: vec![plain_run("3")],
                    },
                ]],
            }
        );
    }

    #[test]
    fn sanitizes_unsafe_links_raw_html_images_and_bare_urls() {
        assert_eq!(
            sanitize_link_url("  https://example.com/a "),
            Some("https://example.com/a".to_string())
        );
        assert_eq!(
            sanitize_link_url("HTTP://example.com"),
            Some("HTTP://example.com".to_string())
        );
        assert_eq!(
            sanitize_link_url("MAILTO:a@b.c"),
            Some("MAILTO:a@b.c".to_string())
        );
        assert_eq!(sanitize_link_url("javascript:alert(1)"), None);
        assert_eq!(sanitize_link_url("data:text/html,x"), None);
        assert_eq!(sanitize_link_url("file:///etc/passwd"), None);
        assert_eq!(sanitize_link_url("https://example.com/\nfoo"), None);
        assert_eq!(sanitize_link_url(""), None);

        let links_doc = parse_markdown(
            "[ok](https://example.com) [mail](MAILTO:a@b.c) [xss](javascript:alert(1)) [data](data:text/html,x)",
        );
        let MdBlock::Paragraph { runs } = &links_doc.blocks[links_doc.root[0] as usize] else {
            panic!("expected Paragraph");
        };
        assert_eq!(
            runs,
            &vec![
                MdRun {
                    text: "ok".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://example.com".to_string()),
                },
                plain_run(" "),
                MdRun {
                    text: "mail".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("MAILTO:a@b.c".to_string()),
                },
                plain_run(" xss data"),
            ]
        );

        let html_doc = parse_markdown("<script>alert(1)</script>");
        assert_eq!(html_doc.root.len(), 1);
        let MdBlock::Paragraph { runs: html_runs } = &html_doc.blocks[html_doc.root[0] as usize]
        else {
            panic!("expected Paragraph for raw HTML");
        };
        assert_eq!(concat_runs(html_runs), "<script>alert(1)</script>");

        let inline_html_doc = parse_markdown("before <img src=x onerror=alert(1)> after");
        let MdBlock::Paragraph {
            runs: inline_html_runs,
        } = &inline_html_doc.blocks[inline_html_doc.root[0] as usize]
        else {
            panic!("expected Paragraph for inline HTML");
        };
        assert_eq!(
            concat_runs(inline_html_runs),
            "before <img src=x onerror=alert(1)> after"
        );

        let img_doc = parse_markdown("![diagram](https://example.com/a.png)");
        assert_eq!(
            img_doc.blocks[img_doc.root[0] as usize],
            MdBlock::Paragraph {
                runs: vec![MdRun {
                    text: "[diagram]".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://example.com/a.png".to_string()),
                }],
            }
        );

        let bare_doc = parse_markdown("see https://example.com/path).");
        assert_eq!(
            bare_doc.blocks[bare_doc.root[0] as usize],
            MdBlock::Paragraph {
                runs: vec![
                    plain_run("see "),
                    MdRun {
                        text: "https://example.com/path".to_string(),
                        bold: false,
                        italic: false,
                        strikethrough: false,
                        code: false,
                        link: Some("https://example.com/path".to_string()),
                    },
                    plain_run(")."),
                ],
            }
        );

        let non_boundary = parse_markdown("foohttps://example.com");
        assert_eq!(
            non_boundary.blocks[non_boundary.root[0] as usize],
            MdBlock::Paragraph {
                runs: vec![plain_run("foohttps://example.com")],
            }
        );
    }

    #[test]
    fn handles_deeply_nested_blockquotes_without_panic() {
        let input = format!("{}leaf", "> ".repeat(64));
        let doc = parse_markdown(&input);
        assert_eq!(doc.root.len(), 1);
        let mut idx = doc.root[0];
        let mut depth = 0usize;
        loop {
            match &doc.blocks[idx as usize] {
                MdBlock::BlockQuote { children } => {
                    assert_eq!(children.len(), 1);
                    depth += 1;
                    idx = children[0];
                }
                MdBlock::Paragraph { runs } => {
                    assert_eq!(runs, &vec![plain_run("leaf")]);
                    break;
                }
                other => panic!("unexpected block in deep blockquote: {other:?}"),
            }
        }
        assert_eq!(depth, 64);
    }

    #[test]
    fn handles_edge_cases_for_empty_input_images_tables_and_urls() {
        let empty = parse_markdown("");
        assert!(empty.blocks.is_empty());
        assert!(empty.root.is_empty());

        let img_empty_alt = parse_markdown(
            "![](https://example.com/a.png) ![](javascript:alert(1)) ![a\nb](https://example.com/b.png) [![badge](https://img.png)](https://target.com)",
        );
        let MdBlock::Paragraph { runs } = &img_empty_alt.blocks[img_empty_alt.root[0] as usize]
        else {
            panic!("expected Paragraph");
        };
        assert_eq!(
            runs,
            &vec![
                MdRun {
                    text: "[https://example.com/a.png]".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://example.com/a.png".to_string()),
                },
                plain_run(" [image] "),
                MdRun {
                    text: "[a b]".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://example.com/b.png".to_string()),
                },
                plain_run(" "),
                MdRun {
                    text: "[badge]".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://target.com".to_string()),
                },
            ]
        );

        let url_cases = parse_markdown(
            "(https://en.wikipedia.org/wiki/Rust_(lang)). **https://example.com** `https://code.invalid` [https://bait.com](javascript:alert(1))",
        );
        let MdBlock::Paragraph { runs: url_runs } = &url_cases.blocks[url_cases.root[0] as usize]
        else {
            panic!("expected Paragraph");
        };
        assert_eq!(
            url_runs,
            &vec![
                plain_run("("),
                MdRun {
                    text: "https://en.wikipedia.org/wiki/Rust_(lang)".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://en.wikipedia.org/wiki/Rust_(lang)".to_string()),
                },
                plain_run("). "),
                MdRun {
                    text: "https://example.com".to_string(),
                    bold: true,
                    italic: false,
                    strikethrough: false,
                    code: false,
                    link: Some("https://example.com".to_string()),
                },
                plain_run(" "),
                MdRun {
                    text: "https://code.invalid".to_string(),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    code: true,
                    link: None,
                },
                plain_run(" https://bait.com"),
            ]
        );

        let header_only_table = parse_markdown("| H1 | H2 |\n| :--- | ---: |");
        assert_eq!(
            header_only_table.blocks[header_only_table.root[0] as usize],
            MdBlock::Table {
                alignments: vec![MdAlign::Left, MdAlign::Right],
                headers: vec![
                    MdTableCell {
                        runs: vec![plain_run("H1")],
                    },
                    MdTableCell {
                        runs: vec![plain_run("H2")],
                    },
                ],
                rows: vec![],
            }
        );
    }
}
