use regex::Regex;

use super::scanner::Parser;
use crate::dsl::model::{Cmp, IsKind, Pred};

impl Parser<'_> {
    pub(super) fn predicate(&mut self) -> Result<Pred, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Pred, String> {
        let mut left = self.parse_and()?;
        while self.try_keyword("or") {
            let right = self.parse_and()?;
            left = match left {
                Pred::Any(mut v) => {
                    v.push(right);
                    Pred::Any(v)
                }
                other => Pred::Any(vec![other, right]),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Pred, String> {
        let mut left = self.parse_not()?;
        while self.try_keyword("and") {
            let right = self.parse_not()?;
            left = match left {
                Pred::All(mut v) => {
                    v.push(right);
                    Pred::All(v)
                }
                other => Pred::All(vec![other, right]),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Pred, String> {
        if self.try_keyword("not") {
            Ok(Pred::Not(Box::new(self.parse_not()?)))
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> Result<Pred, String> {
        self.ws();
        if self.eat('(') {
            let inner = self.parse_or()?;
            self.ws();
            self.expect(')')?;
            return Ok(inner);
        }
        // `@cap`/`$cap` followed by `~`/`!~`/`matches`/`is`.
        if matches!(self.peek(), Some('@') | Some('$')) {
            return self.parse_capture_atom();
        }
        // Function call: `name(@capture)` [`>` number] — a ready-made helper.
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            return self.parse_func();
        }
        Err(format!("expected a condition at byte {}", self.pos))
    }

    /// A capture name preceded by `@` (query capture) or `$` (find metavar) —
    /// both refer to the same named capture.
    fn capture_name(&mut self) -> Result<String, String> {
        self.ws();
        if !(self.eat('@') || self.eat('$')) {
            return Err(format!("expected '@' or '$' at byte {}", self.pos));
        }
        self.ident()
    }

    fn parse_capture_atom(&mut self) -> Result<Pred, String> {
        let capture = self.capture_name()?;
        self.ws();
        // `~ /re/` | `!~ /re/` | `matches /re/`
        let regex_form = if self.rest().starts_with("!~") {
            self.pos += 2;
            Some(true)
        } else if self.eat('~') {
            Some(false)
        } else if self.try_keyword("matches") {
            Some(false)
        } else {
            None
        };
        if let Some(negated) = regex_form {
            self.ws();
            let pattern = self.regex_literal()?;
            let regex = Regex::new(&pattern).map_err(|e| format!("bad regex /{pattern}/: {e}"))?;
            return Ok(Pred::Regex {
                capture,
                regex,
                negated,
            });
        }
        // `is dynamic | constant | user-input`
        if self.try_keyword("is") {
            let kind = match self.ident()?.as_str() {
                "dynamic" => IsKind::Dynamic,
                "constant" => IsKind::Constant,
                "user-input" | "user_input" => IsKind::UserInput,
                other => return Err(format!("unknown condition 'is {other}'")),
            };
            return Ok(Pred::Is { capture, kind });
        }
        Err(format!("expected '~', '!~', 'matches', or 'is' after {capture}"))
    }

    /// `name(@capture)` for a boolean helper, or `name(@capture) > 4` for a
    /// numeric one. Functions are the ready-made `where` helpers.
    fn parse_func(&mut self) -> Result<Pred, String> {
        let name = self.ident()?;
        self.ws();
        self.expect('(')?;
        let capture = self.capture_name()?;
        self.ws();
        self.expect(')')?;

        // optional numeric comparison
        self.ws();
        let cmp = if let Some(op) = self.comparison_op() {
            self.ws();
            Some((op, self.float()?))
        } else {
            None
        };
        Ok(Pred::Func { name, capture, cmp })
    }

    fn comparison_op(&mut self) -> Option<Cmp> {
        let r = self.rest();
        if r.starts_with(">=") {
            self.pos += 2;
            Some(Cmp::Ge)
        } else if r.starts_with("<=") {
            self.pos += 2;
            Some(Cmp::Le)
        } else if r.starts_with('>') {
            self.pos += 1;
            Some(Cmp::Gt)
        } else if r.starts_with('<') {
            self.pos += 1;
            Some(Cmp::Lt)
        } else {
            None
        }
    }

    fn float(&mut self) -> Result<f64, String> {
        self.ws();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        self.s[start..self.pos]
            .parse()
            .map_err(|_| format!("expected a number at byte {}", self.pos))
    }

    /// Read a `/.../`-delimited regex, unescaping only `\/`.
    pub(super) fn regex_literal(&mut self) -> Result<String, String> {
        self.expect('/')?;
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                '/' => return Ok(out),
                '\\' => {
                    if let Some(n) = self.bump() {
                        if n != '/' {
                            out.push('\\'); // keep the backslash; regex needs it
                        }
                        out.push(n);
                    }
                }
                _ => out.push(c),
            }
        }
        Err("unterminated /regex/".into())
    }
}
