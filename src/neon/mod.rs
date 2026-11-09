//! NEON, the format of Nette and PHPStan configuration: a scanner, a parser
//! building a tree with UTF-8 byte spans, and the meaning of scalars. Written
//! after the language-neutral NEON specification and nette/neon's behaviour.

pub mod lexer;
pub mod parser;
pub mod scalar;

pub use lexer::{Lexed, Token, TokenKind, lex};
pub use parser::{
    Array, ArrayKind, Document, Entity, ErrorKind, Item, Neon3, Neon3Note, Role, SyntaxError, Value, parse,
};

/// Problems kept of the scanner, and of the parser, at most: a document of
/// stray characters must not fill the memory with messages nobody reads.
pub const MAX_ERRORS: usize = 1000;

/// A half-open range of UTF-8 byte offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start: start as u32, end: end as u32 }
    }

    pub fn text(self, text: &str) -> &str {
        text.get(self.start as usize..self.end as usize).unwrap_or("")
    }

    pub fn contains(self, offset: usize) -> bool {
        (self.start as usize) <= offset && offset <= self.end as usize
    }

    pub fn to(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    pub fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// A key without Nette's merge suffix `!` and PHPStan's optional prefix `?`.
pub fn strip_key(key: &str) -> &str {
    let key = key.strip_suffix('!').unwrap_or(key);
    key.strip_prefix('?').filter(|k| !k.is_empty()).unwrap_or(key)
}

/// A document scanned and parsed, with what the providers need around it.
pub struct Parsed {
    pub text: String,
    pub lexed: Lexed,
    pub doc: Document,
    pub lines: crate::lines::LineIndex,
}

impl Parsed {
    pub fn new(text: String) -> Self {
        let lexed = lex(&text);
        let doc = parse(&text, &lexed);
        let lines = crate::lines::LineIndex::new(&text);
        Parsed { text, lexed, doc, lines }
    }

    pub fn token_text(&self, index: u32) -> &str {
        self.lexed.tokens.get(index as usize).map_or("", |t| t.span.text(&self.text))
    }

    /// The text a scalar token stands for: a string's value, a literal as written.
    pub fn scalar_text(&self, index: u32) -> std::borrow::Cow<'_, str> {
        let Some(t) = self.lexed.tokens.get(index as usize) else { return "".into() };
        let raw = t.span.text(&self.text);
        match t.kind {
            TokenKind::String => scalar::string_value(raw).unwrap_or(std::borrow::Cow::Borrowed(raw)),
            _ => std::borrow::Cow::Borrowed(raw),
        }
    }

    /// A key's name for lookups: without Nette's `!` (no merging) and PHPStan's `?` (optional).
    pub fn key_name(&self, index: u32) -> std::borrow::Cow<'_, str> {
        match self.scalar_text(index) {
            std::borrow::Cow::Borrowed(s) => std::borrow::Cow::Borrowed(strip_key(s)),
            std::borrow::Cow::Owned(s) => std::borrow::Cow::Owned(strip_key(&s).to_owned()),
        }
    }

    /// The 0-based line of a byte offset.
    pub fn line(&self, offset: u32) -> u32 {
        self.lines.line_of(offset as usize)
    }

    /// The token whose text holds `offset` (a caret right after a token counts).
    pub fn token_at(&self, offset: usize) -> Option<usize> {
        let tokens = &self.lexed.tokens;
        let i = tokens.partition_point(|t| (t.span.end as usize) < offset);
        let hit = |i: usize| tokens.get(i).filter(|t| t.span.contains(offset));
        // Prefer the token starting at the caret over the one ending there,
        // except punctuation, which carries no references.
        match (hit(i), hit(i + 1)) {
            (Some(a), Some(b)) if a.span.end as usize == offset && b.span.start as usize == offset => {
                Some(if matches!(b.kind, TokenKind::Literal | TokenKind::String) { i + 1 } else { i })
            }
            (Some(_), _) => Some(i),
            (None, Some(_)) => Some(i + 1),
            _ => None,
        }
    }
}
