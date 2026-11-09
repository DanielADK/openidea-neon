//! The NEON scanner: tokens with UTF-8 byte spans, comments kept aside as
//! trivia. It follows the scanner of the NEON specification (rules 1–10 of
//! grammar §1.3); where the specification stops at the first unexpected
//! character, this one reports it and goes on with the next one.

use super::{MAX_ERRORS, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A quoted string with its quotes (also `'''` / `"""` multiline ones).
    String,
    /// An unquoted scalar.
    Literal,
    Colon,
    Equals,
    Comma,
    Dash,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    /// Text the scanner could not read: an unterminated string or a stray
    /// character. Its problem is already in [`Lexed::errors`].
    Error,
}

impl TokenKind {
    pub fn is_open(self) -> bool {
        matches!(self, Self::OpenBracket | Self::OpenBrace | Self::OpenParen)
    }

    pub fn is_close(self) -> bool {
        matches!(self, Self::CloseBracket | Self::CloseBrace | Self::CloseParen)
    }

    /// The closing kind of an opening bracket.
    pub fn closer(self) -> Option<Self> {
        match self {
            Self::OpenBracket => Some(Self::CloseBracket),
            Self::OpenBrace => Some(Self::CloseBrace),
            Self::OpenParen => Some(Self::CloseParen),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// A line ending stands between the previous token (or the start) and this one.
    pub newline: bool,
    /// Where this token's line starts (after a byte order mark): for the first
    /// token of a line, `line..span.start` is its indentation.
    pub line: u32,
}

/// A problem of the scanner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    /// What to underline.
    pub span: Span,
    /// Where the specification places the error.
    pub at: u32,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    /// `#` comments, up to the line ending.
    pub comments: Vec<Span>,
    pub errors: Vec<LexError>,
    /// A line ending follows the last token.
    pub eof_newline: bool,
    /// The whitespace after the last line ending, when nothing follows it.
    pub eof_indent: Span,
}

/// Scans the whole text in one pass.
pub fn lex(text: &str) -> Lexed {
    Scanner::new(text).run()
}

struct Scanner<'t> {
    text: &'t str,
    b: &'t [u8],
    i: usize,
    line: usize,
    newline: bool,
    out: Lexed,
    /// For `'''` and `"""`: the last search for a closing line.
    memo: [CloseMemo; 2],
}

/// The first line starting at or after `from` that closes a multiline string
/// (`found`, a line start), or none up to the end. Searches only move forward,
/// so all of them together read the text once.
#[derive(Clone, Copy, Default)]
struct CloseMemo {
    valid: bool,
    from: usize,
    found: Option<usize>,
}

fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

impl<'t> Scanner<'t> {
    fn new(text: &'t str) -> Self {
        let b = text.as_bytes();
        let start = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
        Scanner {
            text,
            b,
            i: start,
            line: start,
            newline: false,
            out: Lexed::default(),
            memo: [CloseMemo::default(); 2],
        }
    }

    fn at(&self, i: usize) -> u8 {
        self.b.get(i).copied().unwrap_or(0)
    }

    /// A line ending (`\n` or `\r\n`) starts at `i`: its length.
    fn line_ending(&self, i: usize) -> usize {
        match self.at(i) {
            b'\n' => 1,
            b'\r' if self.at(i + 1) == b'\n' => 2,
            _ => 0,
        }
    }

    /// A space, a tab or a lone CR (which counts as a space).
    fn blank(&self, i: usize) -> bool {
        i < self.b.len() && (is_space(self.b[i]) || (self.b[i] == b'\r' && self.at(i + 1) != b'\n'))
    }

    fn run(mut self) -> Lexed {
        let n = self.b.len();
        while self.i < n {
            let i = self.i;
            let c = self.b[i];
            let le = self.line_ending(i);
            if le > 0 {
                self.i += le;
                self.line = self.i;
                self.newline = true;
                continue;
            }
            match c {
                b' ' | b'\t' | b'\r' => self.i += 1,
                b'#' => {
                    let mut j = i + 1;
                    while j < n && self.line_ending(j) == 0 {
                        j += 1;
                    }
                    self.out.comments.push(Span::new(i, j));
                    self.i = j;
                }
                b'\'' | b'"' => self.string(),
                b',' => self.single(TokenKind::Comma),
                b'=' => self.single(TokenKind::Equals),
                b'[' => self.single(TokenKind::OpenBracket),
                b']' => self.single(TokenKind::CloseBracket),
                b'{' => self.single(TokenKind::OpenBrace),
                b'}' => self.single(TokenKind::CloseBrace),
                b'(' => self.single(TokenKind::OpenParen),
                b')' => self.single(TokenKind::CloseParen),
                b':' | b'-' => {
                    let next = self.at(i + 1);
                    let quoted_before = i > 0 && matches!(self.b[i - 1], b'"' | b'\'');
                    if i + 1 < n && !ends_short_literal(next) && !quoted_before {
                        let end = self.literal_end(i + 2);
                        self.push(TokenKind::Literal, i, end);
                    } else {
                        self.single(if c == b':' { TokenKind::Colon } else { TokenKind::Dash });
                    }
                }
                b'`' => {
                    // A run of them is one problem.
                    let end = i + self.b[i..].iter().take_while(|&&c| c == b'`').count();
                    self.error(i, end, i);
                    self.push(TokenKind::Error, i, end);
                }
                _ => {
                    let end = self.literal_end(i + 1);
                    self.push(TokenKind::Literal, i, end);
                }
            }
        }
        self.out.eof_newline = self.newline;
        if self.newline || self.out.tokens.is_empty() {
            let line = self.line.min(n);
            if self.b[line..].iter().all(|&c| is_space(c) || c == b'\r') {
                self.out.eof_indent = Span::new(line, n);
            }
        }
        self.out
    }

    fn single(&mut self, kind: TokenKind) {
        let i = self.i;
        self.push(kind, i, i + 1);
    }

    fn push(&mut self, kind: TokenKind, start: usize, end: usize) {
        self.out.tokens.push(Token {
            kind,
            span: Span::new(start, end),
            newline: self.newline,
            line: self.line as u32,
        });
        self.newline = false;
        self.i = end;
        // A multiline string moves to the line of its closing quotes.
        if matches!(kind, TokenKind::String | TokenKind::Error)
            && let Some(last) = self.text[start..end].rfind('\n')
        {
            self.line = start + last + 1;
        }
    }

    /// `Unexpected '…'` at `at`, quoting the rest of the input as nette/neon
    /// does; only the first [`MAX_ERRORS`] are kept.
    fn error(&mut self, start: usize, end: usize, at: usize) {
        if self.out.errors.len() >= MAX_ERRORS {
            return;
        }
        let message = format!("Unexpected '{}'", quote(&self.text[at..]));
        self.out.errors.push(LexError { span: Span::new(start, end), at: at as u32, message });
    }

    /// Continues a literal from `i` (§1.5): the end of the literal.
    fn literal_end(&self, mut i: usize) -> usize {
        let n = self.b.len();
        loop {
            let run = i;
            while i < n && !ends_literal_run(self.b[i]) {
                i += 1;
            }
            if i > run {
                continue;
            }
            if i < n && self.b[i] == b':' && i + 1 < n && !ends_after_colon(self.b[i + 1]) {
                i += 1;
                continue;
            }
            if self.blank(i) {
                let mut j = i;
                while self.blank(j) {
                    j += 1;
                }
                if j < n && !ends_after_blank(self.b[j]) {
                    i = j;
                    continue;
                }
            }
            return i.min(n);
        }
    }

    fn string(&mut self) {
        let i = self.i;
        let q = self.b[i];
        if let Some(end) = self.multiline(i, q) {
            self.push(TokenKind::String, i, end);
            return;
        }
        if self.at(i + 1) == q && self.at(i + 2) == q && self.line_ending(i + 3) > 0 {
            // An opening triple without a closing line: the specification reads
            // it as single-line quotes and fails. The string is taken to end
            // before the next line indented no deeper than its own (as an
            // unclosed bracket is), so the lines below keep their structure.
            let at = if q == b'"' { i + 2 } else { i };
            self.error(i, i + 3, at);
            let end = self.unclosed_end(i + 3);
            self.push(TokenKind::Error, i, end);
            return;
        }
        match self.single_line(i, q) {
            Ok(end) => self.push(TokenKind::String, i, end),
            Err(end) => {
                self.error(i, end, i);
                self.push(TokenKind::Error, i, end);
            }
        }
    }

    /// For a multiline string never closed whose opening triple ends at
    /// `from` (a line ending follows): where it is taken to end, after its
    /// last line with content before the first one indented no deeper than
    /// the line it opened on.
    fn unclosed_end(&self, from: usize) -> usize {
        let n = self.b.len();
        let indent = self.b[self.line..].iter().take_while(|&&c| is_space(c)).count();
        let mut end = from;
        let mut line = from + self.line_ending(from);
        while line < n {
            let eol = self.text[line..].find('\n').map_or(n, |k| line + k);
            let content_end = if eol > line && self.b[eol - 1] == b'\r' { eol - 1 } else { eol };
            let blanks = self.b[line..content_end].iter().take_while(|&&c| is_space(c) || c == b'\r').count();
            if line + blanks < content_end {
                if blanks <= indent {
                    break;
                }
                end = content_end;
            }
            line = eol + 1;
        }
        end
    }

    /// `'…'` or `"…"` on one line: `Ok(end)`, or `Err(end of the line)` when unterminated.
    fn single_line(&self, i: usize, q: u8) -> Result<usize, usize> {
        let n = self.b.len();
        let mut j = i + 1;
        while j < n {
            let c = self.b[j];
            if c == b'\n' {
                break;
            }
            if c == q {
                if q == b'\'' && self.at(j + 1) == b'\'' {
                    j += 2;
                    continue;
                }
                return Ok(j + 1);
            }
            if q == b'"' && c == b'\\' && j + 1 < n && self.line_ending(j + 1) == 0 {
                j += 2;
                continue;
            }
            j += 1;
        }
        let mut end = j.min(n);
        while end < n && self.line_ending(end) == 0 {
            end += 1;
        }
        if end > i + 1 && self.line_ending(end - 1) == 2 {
            end -= 1;
        }
        Err(end)
    }

    /// A `'''` / `"""` string followed by a line ending (§1.4.3): its end.
    fn multiline(&mut self, i: usize, q: u8) -> Option<usize> {
        if self.at(i + 1) != q || self.at(i + 2) != q {
            return None;
        }
        let le = self.line_ending(i + 3);
        if le == 0 {
            return None;
        }
        let first = i + 3 + le;
        let second = self.text[first..].find('\n').map(|k| first + k + 1);
        if let Some(second) = second
            && let Some(close) = self.closing_line(second, q)
        {
            return Some(self.after_triple(close));
        }
        self.closes(first, q).then(|| self.after_triple(first))
    }

    /// The line at `p` starts with optional blanks and then the triple.
    fn closes(&self, p: usize, q: u8) -> bool {
        let mut j = p;
        while self.blank(j) {
            j += 1;
        }
        self.at(j) == q && self.at(j + 1) == q && self.at(j + 2) == q
    }

    fn after_triple(&self, p: usize) -> usize {
        let mut j = p;
        while self.blank(j) {
            j += 1;
        }
        j + 3
    }

    /// The first closing line at or after the line start `p`.
    fn closing_line(&mut self, p: usize, q: u8) -> Option<usize> {
        let m = &self.memo[usize::from(q == b'"')];
        if m.valid && p >= m.from && m.found.is_none_or(|f| f >= p) {
            return m.found;
        }
        let mut line = p;
        let found = loop {
            if line >= self.b.len() {
                break None;
            }
            if self.closes(line, q) {
                break Some(line);
            }
            match self.text[line..].find('\n') {
                Some(k) => line += k + 1,
                None => break None,
            }
        };
        self.memo[usize::from(q == b'"')] = CloseMemo { valid: true, from: p, found };
        found
    }
}

/// After `:` or `-` at the start of a token: the character that makes it a
/// `Colon` or `Dash` instead of a literal.
fn ends_short_literal(c: u8) -> bool {
    matches!(c, b'"' | b'\'' | b',' | b'=' | b'[' | b']' | b'{' | b'}' | b'(' | b')' | b'\n' | b'\r' | b'\t' | b' ')
}

fn ends_literal_run(c: u8) -> bool {
    matches!(c, b',' | b':' | b'=' | b']' | b'}' | b')' | b'(' | b'\n' | b'\r' | b'\t' | b' ')
}

fn ends_after_colon(c: u8) -> bool {
    matches!(c, b'\n' | b'\r' | b'\t' | b' ' | b',' | b']' | b'}' | b')')
}

fn ends_after_blank(c: u8) -> bool {
    matches!(c, b'#' | b',' | b':' | b'=' | b']' | b'}' | b')' | b'(' | b'\n' | b'\r' | b'\t' | b' ')
}

/// Text quoted by `Unexpected '…'`: at most 40 code points, a line ending
/// counting as one and shown as `<new line>`.
pub fn quote(rest: &str) -> String {
    let mut out = String::new();
    let mut chars = rest.chars().peekable();
    let mut count = 0;
    while let Some(c) = chars.next() {
        if count == 40 {
            break;
        }
        count += 1;
        match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                out.push_str("<new line>");
            }
            '\n' => out.push_str("<new line>"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(TokenKind, &str)> {
        lex(text).tokens.iter().map(|t| (t.kind, t.span.text(text))).collect()
    }

    use TokenKind::*;

    #[test]
    fn literals_take_spaces_colons_and_quotes_inside() {
        assert_eq!(kinds("42 px"), [(Literal, "42 px")]);
        assert_eq!(kinds("a:bc"), [(Literal, "a:bc")]);
        assert_eq!(kinds("the\"string#literal"), [(Literal, "the\"string#literal")]);
        assert_eq!(kinds("the\"string #literal"), [(Literal, "the\"string")]);
        assert_eq!(kinds("<literal> <literal>"), [(Literal, "<literal> <literal>")]);
        assert_eq!(kinds("a #b"), [(Literal, "a")]);
        assert_eq!(kinds("a :b"), [(Literal, "a"), (Literal, ":b")]);
        assert_eq!(kinds("abc-"), [(Literal, "abc-")]);
    }

    #[test]
    fn colons_and_dashes_start_literals_before_a_character_of_one() {
        assert_eq!(kinds("::"), [(Literal, "::")]);
        assert_eq!(kinds("-:"), [(Literal, "-:")]);
        assert_eq!(kinds("--1"), [(Literal, "--1")]);
        assert_eq!(kinds(":0"), [(Literal, ":0")]);
        assert_eq!(kinds("- a"), [(Dash, "-"), (Literal, "a")]);
        assert_eq!(kinds("a: b"), [(Literal, "a"), (Colon, ":"), (Literal, "b")]);
        assert_eq!(kinds("{a:b}"), [(OpenBrace, "{"), (Literal, "a:b"), (CloseBrace, "}")]);
        assert_eq!(
            kinds("{a=b}"),
            [(OpenBrace, "{"), (Literal, "a"), (Equals, "="), (Literal, "b"), (CloseBrace, "}")]
        );
        assert_eq!(kinds("'a':b"), [(String, "'a'"), (Colon, ":"), (Literal, "b")]);
    }

    #[test]
    fn nette_expressions_are_literals_and_brackets() {
        assert_eq!(
            kinds("@foo::bar(%a%, 'x')"),
            [
                (Literal, "@foo::bar"),
                (OpenParen, "("),
                (Literal, "%a%"),
                (Comma, ","),
                (String, "'x'"),
                (CloseParen, ")")
            ]
        );
        assert_eq!(
            kinds("Foo()::bar()"),
            [
                (Literal, "Foo"),
                (OpenParen, "("),
                (CloseParen, ")"),
                (Literal, "::bar"),
                (OpenParen, "("),
                (CloseParen, ")")
            ]
        );
        assert_eq!(kinds("App\\Model\\*Factory"), [(Literal, "App\\Model\\*Factory")]);
        assert_eq!(kinds("PDO::ATTR_ERRMODE"), [(Literal, "PDO::ATTR_ERRMODE")]);
    }

    #[test]
    fn strings_and_comments() {
        assert_eq!(kinds("'it''s' \"a\\\"b\""), [(String, "'it''s'"), (String, "\"a\\\"b\"")]);
        let l = lex("a: 1 # note\n# line\nb: 2");
        let comments: Vec<&str> = l.comments.iter().map(|s| s.text("a: 1 # note\n# line\nb: 2")).collect();
        assert_eq!(comments, ["# note", "# line"]);
        assert!(l.tokens[3].newline && !l.tokens[1].newline);
    }

    #[test]
    fn multiline_strings_close_on_a_line_of_their_own() {
        let text = "a: '''\n\tone\n\ttwo\n\t'''\nb: 1";
        assert_eq!(kinds(text)[2], (String, "'''\n\tone\n\ttwo\n\t'''"));
        assert_eq!(kinds(text)[3], (Literal, "b"));
        assert_eq!(kinds("'''\n'''"), [(String, "'''\n'''")]);
        // The first content line is content even when it is the triple.
        assert_eq!(kinds("'''\n'''\n'''"), [(String, "'''\n'''\n'''")]);
        assert_eq!(kinds("\"\"\"\r\nx\r\n\"\"\" ]"), [(String, "\"\"\"\r\nx\r\n\"\"\""), (CloseBracket, "]")]);
        assert_eq!(kinds("'''abc'"), [(String, "'''abc'")]);
    }

    #[test]
    fn unterminated_strings_are_errors_and_scanning_goes_on() {
        let text = "a: \"open\nb: 1";
        let l = lex(text);
        assert_eq!(l.errors[0].message, "Unexpected '\"open<new line>b: 1'");
        assert_eq!(l.errors[0].at, 3);
        assert_eq!(
            kinds(text),
            [(Literal, "a"), (Colon, ":"), (Error, "\"open"), (Literal, "b"), (Colon, ":"), (Literal, "1")]
        );
        let l = lex("x: '''\n  never closed\n");
        assert_eq!(l.tokens.last().map(|t| t.kind), Some(Error));
        assert_eq!(l.errors.len(), 1);
        let l = lex("`x");
        assert_eq!(l.errors[0].message, "Unexpected '`x'");
        let l = lex("a: ```` b");
        assert_eq!((l.tokens.len(), l.errors.len()), (4, 1), "a run of stray characters is one token");
    }

    #[test]
    fn a_multiline_string_never_closed_ends_before_a_line_no_deeper_than_its_own() {
        let text = "parameters:\n\tmessage: '''\n\t\tHello\n\n\t\tWorld\n\nservices:\n\t- App\\Foo\n";
        let l = lex(text);
        assert_eq!(l.errors.len(), 1);
        assert_eq!(l.errors[0].at, text.find("'''").unwrap() as u32);
        let k = kinds(text);
        assert_eq!(k[4], (Error, "'''\n\t\tHello\n\n\t\tWorld"));
        assert_eq!(k[5], (Literal, "services"));
        assert!(l.tokens[5].newline);
        // A sibling key ends it too; content at the key's indentation is no content.
        let k = kinds("a:\n\tb: \"\"\"\r\n\t\tx\r\n\tc: 1");
        assert_eq!(&k[4..], [(Error, "\"\"\"\r\n\t\tx"), (Literal, "c"), (Colon, ":"), (Literal, "1")]);
        assert_eq!(
            kinds("x: '''\ny: 1"),
            [(Literal, "x"), (Colon, ":"), (Error, "'''"), (Literal, "y"), (Colon, ":"), (Literal, "1")]
        );
    }

    #[test]
    fn problems_are_kept_up_to_a_limit() {
        let l = lex(&"` ".repeat(3 * MAX_ERRORS));
        assert_eq!((l.tokens.len(), l.errors.len()), (3 * MAX_ERRORS, MAX_ERRORS));
    }

    #[test]
    fn lines_and_indentation() {
        let text = "\u{feff}a:\n\t\tb: 1\r\n  c";
        let l = lex(text);
        let b = l.tokens[2];
        assert_eq!((b.newline, &text[b.line as usize..b.span.start as usize]), (true, "\t\t"));
        let c = l.tokens[5];
        assert_eq!(&text[c.line as usize..c.span.start as usize], "  ");
        assert_eq!(l.tokens[0].line, 3, "after the byte order mark");
        let l = lex("a:\n\t");
        assert!(l.eof_newline);
        assert_eq!(l.eof_indent.text("a:\n\t"), "\t");
    }

    #[test]
    fn quoting_cuts_at_forty_code_points() {
        assert_eq!(quote(&"ž".repeat(50)), "ž".repeat(40));
        assert_eq!(quote("a\r\nb"), "a<new line>b");
    }
}
