//! One SQL lexer for every validator that has to read a statement the way IRIS does (spec 141,
//! T214, T217, T221).
//!
//! IRIS's SQL lexer, measured on `iris-dev-iris` (2026.2):
//!
//! - A single-quoted literal ends at the next `'` that is not doubled (`''` is one quote). `\` is
//!   an ordinary character in it, so `'a\'` is the literal `a\`.
//! - A double-quoted delimited identifier works the same way with `""`.
//! - `--` starts a line comment and `/*` a block comment only outside a literal or an identifier.
//!   A line comment ends at the newline; a block comment ends at the first `*/` and does not nest.
//! - A literal, identifier or block comment left open is a syntax error in IRIS, so nothing after
//!   it runs.
//!
//! The lexer works on characters and never slices a string by an offset it did not take from the
//! same string, so multibyte text in a literal, an identifier or a comment cannot panic it.
//! Concatenating every token's text gives the input back.

/// What a token is. Comments and white space are tokens so the text can be rebuilt; a validator
/// reads [`code`] to skip them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Space,
    LineComment,
    BlockComment,
    /// `'...'`, quotes included.
    Literal,
    /// `"..."`, quotes included.
    QuotedIdent,
    /// A run of letters, digits and `_`.
    Word,
    /// Any other single character.
    Punct,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    /// The text as written.
    pub text: String,
    /// False for a literal, identifier or block comment that reaches the end of the input without
    /// its closing delimiter.
    pub closed: bool,
}

impl Token {
    /// A word equal to `keyword`, ignoring ASCII case. `keyword` is ASCII.
    pub fn is_word(&self, keyword: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(keyword)
    }

    pub fn is_punct(&self, c: char) -> bool {
        self.kind == Kind::Punct && self.text.chars().eq(std::iter::once(c))
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.kind, Kind::LineComment | Kind::BlockComment)
    }

    /// Neither white space nor a comment.
    pub fn is_code(&self) -> bool {
        !self.is_comment() && self.kind != Kind::Space
    }
}

/// Split `sql` into tokens.
pub fn lex(sql: &str) -> Vec<Token> {
    let chars: Vec<char> = sql.chars().collect();
    let n = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < n {
        let start = i;
        let c = chars[i];
        let (kind, closed) = if c.is_whitespace() {
            while i < n && chars[i].is_whitespace() {
                i += 1;
            }
            (Kind::Space, true)
        } else if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < n && chars[i] != '\n' && chars[i] != '\r' {
                i += 1;
            }
            (Kind::LineComment, true)
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            let mut closed = false;
            while i < n {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            (Kind::BlockComment, closed)
        } else if c == '\'' || c == '"' {
            i += 1;
            let mut closed = false;
            while i < n {
                if chars[i] == c {
                    if chars.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            (
                if c == '\'' {
                    Kind::Literal
                } else {
                    Kind::QuotedIdent
                },
                closed,
            )
        } else if is_word_char(c) {
            while i < n && is_word_char(chars[i]) {
                i += 1;
            }
            (Kind::Word, true)
        } else {
            i += 1;
            (Kind::Punct, true)
        };
        tokens.push(Token {
            kind,
            text: chars[start..i].iter().collect(),
            closed,
        });
    }
    tokens
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The tokens a statement is made of: no white space, no comments.
pub fn code(tokens: &[Token]) -> impl Iterator<Item = &Token> {
    tokens.iter().filter(|t| t.is_code())
}

/// True when a literal, identifier or block comment is left open.
pub fn is_unterminated(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| !t.closed)
}

/// The text of `tokens`, as written.
pub fn text(tokens: &[Token]) -> String {
    tokens.iter().map(|t| t.text.as_str()).collect()
}

/// A dotted SQL name split at the dots that are outside a delimited identifier, each part with its
/// quotes removed and `""` read as `"`: `"A.B".C` is `["A.B", "C"]`.
pub fn split_qualified_name(name: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    for token in lex(name) {
        let part = parts.last_mut().expect("parts is never empty");
        match token.kind {
            Kind::QuotedIdent => {
                let inner: Vec<char> = token.text.chars().collect();
                let end = inner.len() - usize::from(token.closed);
                let body: String = inner[1..end].iter().collect();
                part.push_str(&body.replace("\"\"", "\""));
            }
            Kind::Punct if token.text == "." => parts.push(String::new()),
            _ => part.push_str(&token.text),
        }
    }
    parts
}
