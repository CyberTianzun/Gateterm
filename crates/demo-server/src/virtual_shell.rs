use std::collections::BTreeMap;

const MAX_ENTRIES: usize = 64;
const MAX_FILE: usize = 8192;
const MAX_TOTAL: usize = 65536;
pub const MAX_LINE: usize = 4096;

// None is a directory. These paths never reach any operating-system API.
pub struct VirtualFs {
    entries: BTreeMap<String, Option<String>>,
}

impl Default for VirtualFs {
    fn default() -> Self {
        Self { entries: BTreeMap::from([
            ("/".into(), None), ("/workspace".into(), None),
            ("/workspace/README.txt".into(), Some("Virtual demo — no host shell / no real files\nTry mkdir notes, echo 'hello' > notes/hello.txt, cat notes/hello.txt\n".into())),
            ("/workspace/example.txt".into(), Some("All files exist only in memory until the demo restarts.\n".into())),
        ]) }
    }
}

impl VirtualFs {
    pub fn directory(&self, path: &str) -> Option<String> {
        let path = normalize("/workspace", path).ok()?;
        matches!(self.entries.get(&path), Some(None)).then_some(path)
    }

    fn write(&mut self, path: String, text: &str, append: bool) -> Result<(), String> {
        let previous = match self.entries.get(&path) {
            Some(Some(text)) => text.as_str(),
            Some(None) => return Err("is a directory".into()),
            None => "",
        };
        let size = text.len() + if append { previous.len() } else { 0 };
        let total: usize = self
            .entries
            .values()
            .filter_map(Option::as_ref)
            .map(String::len)
            .sum();
        if size > MAX_FILE || total - previous.len() + size > MAX_TOTAL {
            return Err("virtual file quota exceeded".into());
        }
        self.check_new(&path)?;
        let content = if append {
            format!("{previous}{text}")
        } else {
            text.to_owned()
        };
        self.entries.insert(path, Some(content));
        Ok(())
    }

    fn check_new(&self, path: &str) -> Result<(), String> {
        if !self.entries.contains_key(path) && self.entries.len() >= MAX_ENTRIES {
            return Err("virtual entry quota exceeded".into());
        }
        let parent = path
            .rsplit_once('/')
            .map(|(p, _)| if p.is_empty() { "/" } else { p })
            .unwrap_or("/");
        if !matches!(self.entries.get(parent), Some(None)) {
            return Err("parent directory does not exist".into());
        }
        Ok(())
    }
}

fn normalize(cwd: &str, path: &str) -> Result<String, String> {
    if path.len() > 1024
        || path
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
    {
        return Err("invalid virtual path".into());
    }
    let joined = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{cwd}/{path}")
    };
    let mut parts = vec![];
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("path leaves virtual root".into());
                }
            }
            p if p.len() > 255 => return Err("virtual name too long".into()),
            p => parts.push(p),
        }
    }
    let result = format!("/{}", parts.join("/"));
    if result.len() > 1024 {
        return Err("virtual path too long".into());
    }
    Ok(result)
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Redirect(bool),
}

fn parse(line: &str) -> Result<Vec<Token>, String> {
    let mut tokens = vec![];
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if tokens.len() >= 64 {
            return Err("at most 64 command tokens are allowed".into());
        }
        if c == '\\' && quote != Some('\'') {
            word.push(chars.next().ok_or("unfinished escape")?);
            started = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            started = true;
        } else if c.is_whitespace() {
            if started {
                tokens.push(Token::Word(std::mem::take(&mut word)));
                started = false;
            }
        } else if c == '>' {
            if started {
                tokens.push(Token::Word(std::mem::take(&mut word)));
                started = false;
            }
            let append = chars.peek() == Some(&'>');
            if append {
                chars.next();
            }
            tokens.push(Token::Redirect(append));
        } else if matches!(c, '|' | '&' | ';' | '$' | '`' | '<' | '(' | ')') {
            return Err("pipes, chains and substitutions are unsupported".into());
        } else {
            word.push(c);
            started = true;
        }
    }
    if quote.is_some() {
        return Err("unclosed quote".into());
    }
    if started {
        tokens.push(Token::Word(word));
    }
    if tokens.len() > 64 {
        return Err("at most 64 command tokens are allowed".into());
    }
    Ok(tokens)
}

pub struct Shell {
    pub review: Option<crate::review::Review>,
    pub cwd: String,
    line: String,
    pending: Vec<u8>,
    invalid: bool,
    cr: bool,
    escape: bool,
}

impl Shell {
    pub fn new(cwd: &str) -> Self {
        Self {
            review: None,
            cwd: cwd.into(),
            line: String::new(),
            pending: Vec::with_capacity(4),
            invalid: false,
            cr: false,
            escape: false,
        }
    }

    pub fn prompt(&self) -> String {
        format!("virtual:{}$ ", self.cwd)
    }

    pub fn input(&mut self, fs: &mut VirtualFs, bytes: &[u8]) -> Vec<u8> {
        let mut out = String::new();
        for &b in bytes.iter().take(65536) {
            if out.len() > 32768 {
                self.line.clear();
                self.pending.clear();
                self.invalid = false;
                out.push_str(
                    "\r\ndemo: frame output budget exceeded; remaining input discarded\r\n",
                );
                out.push_str(&self.prompt());
                break;
            }
            if self.escape {
                // Ignore a bounded VT key sequence; never echo control bytes.
                if b.is_ascii_alphabetic() || b == b'~' {
                    self.escape = false;
                }
                continue;
            }
            if b == 27 {
                self.escape = true;
                continue;
            }
            if b == b'\n' && self.cr {
                self.cr = false;
                continue;
            }
            self.cr = b == b'\r';
            match b {
                b'\r' | b'\n' => {
                    out.push_str("\r\n");
                    if self.invalid || !self.pending.is_empty() {
                        out.push_str("demo: invalid or excessive input line\r\n");
                    } else {
                        match self.execute(fs) {
                            Ok(text) => out.push_str(&text.replace('\n', "\r\n")),
                            Err(error) => out.push_str(&format!("demo: {error}\r\n")),
                        }
                    }
                    self.line.clear();
                    self.pending.clear();
                    self.invalid = false;
                    out.push_str(&self.prompt());
                }
                3 => {
                    self.line.clear();
                    self.pending.clear();
                    self.invalid = false;
                    out.push_str("^C\r\n");
                    out.push_str(&self.prompt());
                }
                12 => {
                    out.push_str("\x1b[2J\x1b[H");
                    out.push_str(&self.prompt());
                    out.push_str(&self.line);
                }
                8 | 127 => {
                    if !self.pending.is_empty() {
                        self.pending.clear();
                    } else if self.line.pop().is_some() {
                        // Redraw the line: handles wide and combining Unicode without byte deletion.
                        out.push_str("\r\x1b[2K");
                        out.push_str(&self.prompt());
                        out.push_str(&self.line);
                    }
                }
                0..=31 => {
                    self.invalid = true;
                }
                _ if self.invalid => {}
                _ => {
                    self.pending.push(b);
                    match std::str::from_utf8(&self.pending) {
                        Ok(text) => {
                            if self.line.len() + text.len() > MAX_LINE
                                || text.chars().any(char::is_control)
                            {
                                self.invalid = true;
                            } else {
                                self.line.push_str(text);
                                out.push_str(text);
                            }
                            self.pending.clear();
                        }
                        Err(err) if err.error_len().is_some() || self.pending.len() >= 4 => {
                            self.invalid = true;
                            self.pending.clear();
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        out.into_bytes()
    }

    fn execute(&mut self, fs: &mut VirtualFs) -> Result<String, String> {
        let tokens = parse(&self.line)?;
        let mut args = vec![];
        let mut redirect = None;
        let mut iter = tokens.into_iter();
        while let Some(token) = iter.next() {
            match token {
                Token::Word(word) => args.push(word),
                Token::Redirect(append) => {
                    let Some(Token::Word(path)) = iter.next() else {
                        return Err("redirection requires one path".into());
                    };
                    if iter.next().is_some() {
                        return Err("redirection must end the command".into());
                    }
                    redirect = Some((path, append));
                }
            }
        }
        let Some(cmd) = args.first().map(String::as_str) else {
            return Ok(String::new());
        };
        if redirect.is_some() && cmd != "echo" {
            return Err("only echo supports redirection".into());
        }
        let arity = |min, max| {
            if (min..=max).contains(&(args.len() - 1)) {
                Ok(())
            } else {
                Err("wrong number of arguments".to_string())
            }
        };
        match cmd {
            "help" => {
                arity(0, 0)?;
                Ok("Virtual demo — no host shell / no real files\nCommands: help pwd ls cd mkdir touch cat echo clear\nIn the simulated review terminal: review, review allow, review deny.\necho supports > and >> virtual files. No pipes/chains/substitutions.\n".into())
            }
            "review" => self
                .review
                .as_mut()
                .ok_or("open the Simulated coding-agent review terminal")?
                .command(&args[1..]),
            "pwd" => {
                arity(0, 0)?;
                Ok(format!("{}\n", self.cwd))
            }
            "clear" => {
                arity(0, 0)?;
                Ok("\x1b[2J\x1b[H".into())
            }
            "cd" => {
                arity(0, 1)?;
                let path = normalize(
                    &self.cwd,
                    args.get(1).map(String::as_str).unwrap_or("/workspace"),
                )?;
                if !matches!(fs.entries.get(&path), Some(None)) {
                    return Err("no such virtual directory".into());
                }
                self.cwd = path;
                Ok(String::new())
            }
            "ls" => {
                arity(0, 1)?;
                let path = normalize(&self.cwd, args.get(1).map(String::as_str).unwrap_or("."))?;
                if !matches!(fs.entries.get(&path), Some(None)) {
                    return Err("no such virtual directory".into());
                }
                let prefix = if path == "/" {
                    "/".into()
                } else {
                    format!("{path}/")
                };
                let mut out = String::new();
                for (p, content) in &fs.entries {
                    if let Some(name) = p
                        .strip_prefix(&prefix)
                        .filter(|n| !n.is_empty() && !n.contains('/'))
                    {
                        out.push_str(name);
                        if content.is_none() {
                            out.push('/');
                        }
                        out.push('\n');
                    }
                }
                Ok(out)
            }
            "mkdir" | "touch" | "cat" => {
                arity(1, 1)?;
                let path = normalize(&self.cwd, &args[1])?;
                match cmd {
                    "cat" => fs
                        .entries
                        .get(&path)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or("no such virtual file".into()),
                    "mkdir" => {
                        if fs.entries.contains_key(&path) {
                            return Err("virtual path already exists".into());
                        }
                        fs.check_new(&path)?;
                        fs.entries.insert(path, None);
                        Ok(String::new())
                    }
                    _ => {
                        fs.write(path, "", true)?;
                        Ok(String::new())
                    }
                }
            }
            "echo" => {
                let text = format!("{}\n", args[1..].join(" "));
                if let Some((path, append)) = redirect {
                    fs.write(normalize(&self.cwd, &path)?, &text, append)?;
                    Ok(String::new())
                } else {
                    Ok(text)
                }
            }
            _ => Err("unsupported virtual command (try help)".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(sh: &mut Shell, fs: &mut VirtualFs, line: &str) -> String {
        String::from_utf8(sh.input(fs, format!("{line}\r").as_bytes())).unwrap()
    }

    #[test]
    fn quoted_virtual_file_workflow() {
        let mut fs = VirtualFs::default();
        let mut sh = Shell::new("/workspace");
        run(&mut sh, &mut fs, "mkdir 'my docs'");
        run(&mut sh, &mut fs, "cd 'my docs'");
        assert!(run(&mut sh, &mut fs, "pwd").contains("/workspace/my docs\r\n"));
        run(&mut sh, &mut fs, "touch empty");
        run(&mut sh, &mut fs, "echo 'hello world' > note");
        run(&mut sh, &mut fs, "echo again >> note");
        assert!(run(&mut sh, &mut fs, "cat note").contains("hello world\r\nagain\r\n"));
        assert!(run(&mut sh, &mut fs, "ls").contains("empty"));
    }

    #[test]
    fn invalid_syntax_and_host_paths_never_dispatch() {
        let mut fs = VirtualFs::default();
        let mut sh = Shell::new("/workspace");
        for line in [
            "cat /etc/passwd",
            "cat ../../etc/passwd",
            "cat C:\\Windows\\win.ini",
            "echo x; touch attack",
            "echo $(whoami)",
            "echo `whoami`",
            "echo x | cat",
            "echo x && cat",
            "/bin/sh",
            "echo x > /../../attack",
            "touch bad\0name",
        ] {
            assert!(run(&mut sh, &mut fs, line).contains("demo: "), "{line}");
        }
        assert!(run(&mut sh, &mut fs, "cat attack").contains("demo: "));
        run(&mut sh, &mut fs, "echo '$(whoami); safe' > literal");
        assert!(run(&mut sh, &mut fs, "cat literal").contains("$(whoami); safe\r\n"));
    }

    #[test]
    fn quotas_reject_transactionally() {
        let mut fs = VirtualFs::default();
        let mut sh = Shell::new("/workspace");
        run(&mut sh, &mut fs, "echo original > note");
        let chunk = "x".repeat(4000);
        run(&mut sh, &mut fs, &format!("echo {chunk} >> note"));
        run(&mut sh, &mut fs, &format!("echo {chunk} >> note"));
        assert!(run(&mut sh, &mut fs, &format!("echo {chunk} >> note")).contains("demo: "));
        assert!(run(&mut sh, &mut fs, "cat note").contains("original\r\n"));
        for n in 0..70 {
            run(&mut sh, &mut fs, &format!("touch f{n}"));
        }
        assert!(run(&mut sh, &mut fs, "touch final").contains("demo: "));
        assert!(run(&mut sh, &mut fs, "cat final").contains("demo: "));
    }

    #[test]
    fn unicode_split_and_basic_editor_are_bounded() {
        let mut fs = VirtualFs::default();
        let mut sh = Shell::new("/workspace");
        let mut out = vec![];
        for b in "echo 你好🙂\r\n".as_bytes() {
            out.extend(sh.input(&mut fs, &[*b]));
        }
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("你好🙂\r\n"));
        assert_eq!(out.matches("virtual:").count(), 1, "CRLF is one submission");
        let out = sh.input(&mut fs, "echo 🙂\u{7f}ok\r".as_bytes());
        assert!(String::from_utf8(out).unwrap().contains("ok\r\n"));
        sh.input(&mut fs, b"touch never\x03");
        assert!(run(&mut sh, &mut fs, "cat never").contains("demo: "));
        assert!(
            run(
                &mut sh,
                &mut fs,
                &format!("echo {} > oversized", "x".repeat(5000))
            )
            .contains("demo: ")
        );
        assert!(run(&mut sh, &mut fs, "cat oversized").contains("demo: "));
    }

    #[test]
    fn excessive_arguments_and_command_output_cannot_amplify_a_frame() {
        let mut fs = VirtualFs::default();
        let mut sh = Shell::new("/workspace");
        assert!(run(&mut sh, &mut fs, &format!("echo {}", "x ".repeat(65))).contains("demo: "));
        let bytes = sh.input(&mut fs, "ls\r".repeat(16000).as_bytes());
        assert!(bytes.len() <= 65536);
        assert!(String::from_utf8_lossy(&bytes).contains("remaining input discarded"));
    }

    #[test]
    fn total_quota_and_invalid_overwrites_preserve_original_content() {
        let mut fs = VirtualFs::default();
        fs.write("/workspace/note".into(), "kept", false).unwrap();
        for n in 0..7 {
            fs.write(format!("/workspace/full{n}"), &"a".repeat(8192), false)
                .unwrap();
        }
        assert!(
            fs.write("/workspace/note".into(), &"b".repeat(8192), false)
                .is_err()
        );
        assert_eq!(fs.entries["/workspace/note"].as_deref(), Some("kept"));
        assert!(fs.write("/workspace/full0".into(), "x", true).is_err());
        assert_eq!(fs.entries["/workspace/full0"].as_ref().unwrap().len(), 8192);
    }
}
