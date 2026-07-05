//! Low-level cursor over the source: whitespace/comments, identifiers, strings,
//! numbers, and single-character matching. Shared by all parser submodules.

pub(super) struct Parser<'a> {
    pub(super) s: &'a str,
    pub(super) pos: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn new(s: &'a str) -> Self {
        Parser { s, pos: 0 }
    }

    pub(super) fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(super) fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Skip whitespace and `//` / `#` line comments.
    pub(super) fn ws(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => self.pos += c.len_utf8(),
                Some('/') if self.rest().starts_with("//") => self.skip_line(),
                Some('#') => self.skip_line(),
                _ => break,
            }
        }
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
        }
    }

    /// Consume the rest of the current line (after skipping leading spaces/tabs)
    /// and return it trimmed. Used for `find:`/`examples` snippets, which are
    /// taken literally — no comment stripping, so a snippet may itself be `//…`.
    pub(super) fn rest_of_line(&mut self) -> String {
        while matches!(self.peek(), Some(' ') | Some('\t')) {
            self.pos += 1;
        }
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.pos += c.len_utf8();
        }
        self.s[start..self.pos].trim_end().to_string()
    }

    pub(super) fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// Read a `{ … }` body and return its contents trimmed. Assumes the opening
    /// `{` has already been consumed. Tracks string literals (`"`, `'`, `` ` ``)
    /// so braces inside code (objects, function bodies) are balanced correctly.
    pub(super) fn brace_body(&mut self) -> Result<String, String> {
        let start = self.pos;
        let mut depth = 1i32;
        let mut in_str: Option<char> = None;
        while let Some(c) = self.peek() {
            if let Some(q) = in_str {
                self.pos += c.len_utf8();
                if c == '\\' {
                    if let Some(n) = self.peek() {
                        self.pos += n.len_utf8();
                    }
                } else if c == q {
                    in_str = None;
                }
                continue;
            }
            match c {
                '"' | '\'' | '`' => {
                    in_str = Some(c);
                    self.pos += c.len_utf8();
                }
                '{' => {
                    depth += 1;
                    self.pos += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let body = self.s[start..self.pos].trim().to_string();
                        self.pos += 1; // consume the closing '}'
                        return Ok(body);
                    }
                    self.pos += 1;
                }
                _ => self.pos += c.len_utf8(),
            }
        }
        Err("unterminated find { … } block".into())
    }

    pub(super) fn expect(&mut self, c: char) -> Result<(), String> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(format!("expected '{c}' at byte {} (got {:?})", self.pos, self.peek()))
        }
    }

    pub(super) fn ident(&mut self) -> Result<String, String> {
        self.ws();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(format!("expected identifier at byte {}", self.pos));
        }
        Ok(self.s[start..self.pos].to_string())
    }

    /// Consume `kw` if it's the next identifier; otherwise leave position intact.
    pub(super) fn try_keyword(&mut self, kw: &str) -> bool {
        let save = self.pos;
        self.ws();
        match self.ident() {
            Ok(id) if id == kw => true,
            _ => {
                self.pos = save;
                false
            }
        }
    }

    pub(super) fn string(&mut self) -> Result<String, String> {
        self.ws();
        self.expect('"')?;
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                '"' => return Ok(out),
                '\\' => {
                    if let Some(n) = self.bump() {
                        out.push(match n {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    }
                }
                _ => out.push(c),
            }
        }
        Err("unterminated string".into())
    }

    pub(super) fn number(&mut self) -> Result<usize, String> {
        self.ws();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(format!("expected number at byte {}", self.pos));
        }
        self.s[start..self.pos].parse().map_err(|_| "bad number".to_string())
    }
}
