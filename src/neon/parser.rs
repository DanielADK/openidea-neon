//! The NEON parser (specification, grammar §2): block arrays by indentation,
//! inline arrays, entities and chains, with the checks of keys a decoder
//! makes. Unlike nette/neon it does not stop at the first problem: it reports
//! it, skips the rest of the line (or the token, inside brackets) and goes
//! on, so one mistake leaves the rest of the tree intact. Linear in the input.

use super::lexer::{Lexed, Token, TokenKind, quote};
use super::scalar;
use super::{MAX_ERRORS, Span};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Arrays nest at most this deep (§2.7); deeper ones are reported and skipped.
pub const MAX_DEPTH: u32 = 500;

#[derive(Debug)]
pub enum Value {
    /// A literal, a quoted string or an unreadable token: its index.
    Scalar(u32),
    Array(Array),
    Entity(Box<Entity>),
    /// Two or more entities: `Foo()::bar()`.
    Chain(Vec<Entity>),
}

#[derive(Debug)]
pub struct Entity {
    pub head: Value,
    /// `None` for a chain link without parentheses.
    pub attributes: Option<Array>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    Block,
    /// `[…]`
    Bracket,
    /// `{…}`
    Brace,
    /// `(…)`: the attributes of an entity, or an inline list.
    Paren,
}

#[derive(Debug)]
pub struct Array {
    pub kind: ArrayKind,
    pub items: Vec<Item>,
    pub span: Span,
    /// The closing bracket was found (always true for blocks).
    pub closed: bool,
}

#[derive(Debug)]
pub struct Item {
    /// The `-` token.
    pub dash: Option<u32>,
    /// The key token (a literal or a string).
    pub key: Option<u32>,
    pub value: Option<Value>,
    pub span: Span,
}

impl Value {
    pub fn span(&self, tokens: &[Token]) -> Span {
        match self {
            Value::Scalar(i) => tokens.get(*i as usize).map_or_else(Span::default, |t| t.span),
            Value::Array(a) => a.span,
            Value::Entity(e) => e.span,
            Value::Chain(links) => match (links.first(), links.last()) {
                (Some(a), Some(b)) => a.span.to(b.span),
                _ => Span::default(),
            },
        }
    }

    pub fn as_array(&self) -> Option<&Array> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn scalar(&self) -> Option<u32> {
        match self {
            Value::Scalar(i) => Some(*i),
            _ => None,
        }
    }
}

/// What a token stands for in the tree, for highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
    Value,
    Key,
    /// The name of an entity: `Foo` in `Foo(…)`.
    EntityHead,
    /// A later link of a chain: `::bar` in `Foo()::bar()`.
    ChainLink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// A token or character the grammar does not allow there.
    Syntax,
    BadIndentation,
    TabsAndSpaces,
    DuplicateKey,
    UnacceptableKey,
    /// A bracket without its closing pair.
    Unclosed,
    Escape,
    /// `\xHH` in an otherwise valid string: refused since nette/neon 3.4,
    /// read by 3.3 (PHPStan), deprecated.
    HexEscape,
    TooDeep,
    ImplicitKeyOverflow,
    ReservedEntity,
}

impl ErrorKind {
    /// The code shown with the problem.
    pub fn code(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::BadIndentation => "bad-indentation",
            Self::TabsAndSpaces => "tabs-and-spaces",
            Self::DuplicateKey => "duplicate-key",
            Self::UnacceptableKey => "unacceptable-key",
            Self::Unclosed => "unclosed",
            Self::Escape | Self::HexEscape => "invalid-escape",
            Self::TooDeep => "too-deep",
            Self::ImplicitKeyOverflow => "implicit-key-overflow",
            Self::ReservedEntity => "reserved-entity",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub kind: ErrorKind,
    pub message: String,
    /// What to underline.
    pub span: Span,
    /// Where nette/neon reports it (the start of the offending token, a line
    /// ending, or the end of the input).
    pub at: u32,
    /// Another place that explains it (the first of two duplicate keys, the
    /// opening bracket).
    pub related: Option<(Span, &'static str)>,
}

/// What nette/neon 3 reads differently from 4.0.1, whose reading this parser
/// follows. PHPStan 2 bundles nette/neon 3.3; Nette 3 projects mostly have 3.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Neon3 {
    /// A value on the line after its key inside brackets: 3.x reads the key
    /// as null and the value as an item of its own.
    ValueOnNextLine,
    /// The colon or equals sign of a key inside brackets on a later line than
    /// the key: 3.x refuses it.
    ColonOnNextLine,
    /// A comma first on its line inside brackets: 3.3 refuses it.
    CommaOnNextLine,
    /// A tab indenting the lines of a block opened on the line of a dash
    /// (`- a: 1` + `\t\tb: 2`): 3.3 expects two spaces there and refuses it.
    TabAfterDash,
    /// `on` or `off` as a value: a boolean in 3.3 (deprecated), a string since 3.4.
    OnOff,
    /// A byte order mark: 3.x reads it as part of the first key or value.
    ByteOrderMark,
}

impl Neon3 {
    /// Only nette/neon 3.3 reads it differently; 3.4 agrees with 4.0.1.
    pub fn only_3_3(self) -> bool {
        !matches!(self, Self::ValueOnNextLine | Self::ColonOnNextLine | Self::ByteOrderMark)
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::ValueOnNextLine => "neon3-value-on-next-line",
            Self::ColonOnNextLine => "neon3-colon-on-next-line",
            Self::CommaOnNextLine => "neon3-comma-on-next-line",
            Self::TabAfterDash => "neon3-tab-after-dash",
            Self::OnOff => "neon3-on-off",
            Self::ByteOrderMark => "neon3-byte-order-mark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Neon3Note {
    pub kind: Neon3,
    pub span: Span,
    /// The key the value belongs to ([`Neon3::ValueOnNextLine`]).
    pub key: Option<u32>,
}

#[derive(Debug, Default)]
pub struct Document {
    /// The value of the document: usually a block array.
    pub root: Option<Value>,
    /// Blocks read after a problem that ended the root early (recovery).
    pub rest: Vec<Value>,
    pub errors: Vec<SyntaxError>,
    /// Where nette/neon 3 reads the document otherwise (no problem for 4.0.1).
    pub neon3: Vec<Neon3Note>,
    /// One per token.
    pub roles: Vec<Role>,
}

impl Document {
    /// The root and the recovered blocks.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.root.iter().chain(&self.rest)
    }

    pub fn role(&self, token: usize) -> Role {
        self.roles.get(token).copied().unwrap_or(Role::Value)
    }
}

/// Parses scanned text; the scanner's problems come first in the errors.
pub fn parse(text: &str, lexed: &Lexed) -> Document {
    let errors: Vec<SyntaxError> = lexed
        .errors
        .iter()
        .map(|e| SyntaxError {
            kind: ErrorKind::Syntax,
            message: e.message.clone(),
            span: e.span,
            at: e.at,
            related: None,
        })
        .collect();
    let mut p = Parser {
        text,
        toks: &lexed.tokens,
        eof_newline: lexed.eof_newline,
        eof_indent: lexed.eof_indent.text(text),
        pos: 0,
        taken: false,
        limit: lexed.tokens.len(),
        depth: 0,
        recovering: 0,
        pairs: pairs(&lexed.tokens),
        max_errors: errors.len() + MAX_ERRORS,
        errors,
        neon3: Vec::new(),
        roles: vec![Role::Value; lexed.tokens.len()],
    };
    if text.starts_with('\u{feff}') && !lexed.tokens.is_empty() {
        p.note(Neon3::ByteOrderMark, Span::new(0, 3), None);
    }
    let (root, rest) = p.document();
    p.on_off();
    Document { root, rest, errors: p.errors, neon3: p.neon3, roles: p.roles }
}

/// The partner of each opening bracket: its closing bracket, or for one never
/// closed the first later line indented no deeper than the bracket's own line,
/// where its content is taken to end (bit 31 set). A closing bracket pairs
/// with the nearest open one of its kind, closing those above it; one with
/// none open of its kind is skipped without a search, which keeps this linear.
fn pairs(toks: &[Token]) -> Vec<u32> {
    const UNSET: u32 = u32::MAX;
    let mut out = vec![UNSET; toks.len()];
    let mut stack: Vec<usize> = Vec::new();
    let kind = |k: TokenKind| match k {
        TokenKind::OpenBracket | TokenKind::CloseBracket => 0,
        TokenKind::OpenBrace | TokenKind::CloseBrace => 1,
        _ => 2,
    };
    let mut open = [0usize; 3]; // brackets of each kind on the stack
    let mut unclosed: Vec<(usize, usize)> = Vec::new(); // (opener, its line's indentation)
    let mut line_indent = 0usize;
    let mut indent_of_line = Vec::with_capacity(toks.len().min(1 << 16)); // (token, indentation) of line starts
    for (i, t) in toks.iter().enumerate() {
        if i == 0 || t.newline {
            line_indent = (t.span.start - t.line) as usize;
            indent_of_line.push((i, line_indent));
        }
        if t.kind.is_open() {
            stack.push(i);
            open[kind(t.kind)] += 1;
            out[i] = line_indent as u32; // the indentation, until the opener is resolved
        } else if t.kind.is_close()
            && open[kind(t.kind)] > 0
            && let Some(depth) = stack.iter().rposition(|&o| toks[o].kind.closer() == Some(t.kind))
        {
            for &o in &stack[depth..] {
                open[kind(toks[o].kind)] -= 1;
            }
            for &o in &stack[depth + 1..] {
                unclosed.push((o, out[o] as usize));
            }
            out[stack[depth]] = i as u32;
            stack.truncate(depth);
        }
    }
    for &o in &stack {
        unclosed.push((o, out[o] as usize));
    }
    unclosed.sort_unstable();
    // The cut of each unclosed bracket: the first later line start indented no
    // deeper. Overlapping searches only happen for ever deeper lines, which
    // keeps the total work linear in the text.
    let mut last: Option<(usize, usize, usize)> = None; // (from, indent, cut): brackets of one line share it
    for (o, indent) in unclosed {
        let from = indent_of_line.partition_point(|&(i, _)| i <= o);
        let cut = match last {
            Some((f, ind, cut)) if f == from && ind == indent => cut,
            _ => indent_of_line[from..].iter().find(|&&(_, ind)| ind <= indent).map_or(toks.len(), |&(i, _)| i),
        };
        last = Some((from, indent, cut));
        out[o] = cut as u32 | CUT;
    }
    out
}

const CUT: u32 = 1 << 31;

/// The indentation a block expects: known, or (for a block opened on the line
/// of a dash) decided by the next line (§2.4).
#[derive(Clone)]
enum Pending<'t> {
    Known(Cow<'t, str>),
    Dash(Rc<RefCell<DashIndent<'t>>>),
}

struct DashIndent<'t> {
    base: Pending<'t>,
    resolved: Option<Rc<str>>,
    /// The first token of the line that decided it, and the number of problems then.
    line: usize,
    errors: usize,
}

/// Explicit keys of one array: duplicates and the implicit keys after them.
#[derive(Default)]
struct Keys<'t> {
    small: Vec<(Cow<'t, str>, u32)>,
    large: Option<HashSet<Cow<'t, str>>>,
    max: Option<i64>,
    /// Float keys nette/neon 3 and 4.0.1 round differently, by value
    /// ([`scalar::is_float_tie`]): duplicates of the same number only.
    ties: Vec<(u64, u32)>,
}

struct Parser<'t> {
    text: &'t str,
    toks: &'t [Token],
    eof_newline: bool,
    eof_indent: &'t str,
    pos: usize,
    /// The line ending before `pos` was consumed.
    taken: bool,
    /// Tokens from here on are out of reach (the end of an unclosed bracket).
    limit: usize,
    depth: u32,
    /// Inside lines read after `Bad indentation`, as a block of their own.
    recovering: u32,
    pairs: Vec<u32>,
    /// Problems are kept up to this many.
    max_errors: usize,
    errors: Vec<SyntaxError>,
    neon3: Vec<Neon3Note>,
    roles: Vec<Role>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    Token(TokenKind),
    Newline,
    End,
}

impl<'t> Parser<'t> {
    // ---- the token stream with its virtual line endings ----

    fn peek(&self) -> Next {
        let nl = match self.toks.get(self.pos) {
            Some(t) => t.newline,
            None => self.eof_newline,
        };
        if nl && !self.taken {
            Next::Newline
        } else if self.pos >= self.limit {
            Next::End
        } else {
            Next::Token(self.toks[self.pos].kind)
        }
    }

    fn is_any(&self) -> bool {
        self.peek() != Next::End
    }

    fn is(&self, kind: TokenKind) -> bool {
        self.peek() == Next::Token(kind)
    }

    fn try_newline(&mut self) -> bool {
        let nl = self.peek() == Next::Newline;
        if nl {
            self.taken = true;
        }
        nl
    }

    fn bump(&mut self) -> usize {
        self.taken = false;
        self.pos += 1;
        self.pos - 1
    }

    fn try_consume(&mut self, kinds: &[TokenKind]) -> Option<usize> {
        match self.peek() {
            Next::Token(k) if kinds.contains(&k) => Some(self.bump()),
            _ => None,
        }
    }

    fn mark(&self) -> (usize, bool) {
        (self.pos, self.taken)
    }

    fn reset(&mut self, mark: (usize, bool)) {
        (self.pos, self.taken) = mark;
    }

    /// The indentation of the next token when it starts a line.
    fn indentation(&self) -> &'t str {
        match self.toks.get(self.pos) {
            Some(t) if self.pos == 0 || t.newline => &self.text[t.line as usize..t.span.start as usize],
            Some(_) => "",
            None => self.eof_indent,
        }
    }

    fn span_of(&self, i: usize) -> Span {
        self.toks.get(i).map_or(Span::new(self.text.len(), self.text.len()), |t| t.span)
    }

    fn value_span(&self, v: &Value) -> Span {
        v.span(self.toks)
    }

    // ---- problems ----

    fn error(&mut self, kind: ErrorKind, message: String, span: Span, at: u32) {
        // Recovery can meet one place twice (two blocks ending on one line).
        if self.errors.last().is_some_and(|e| e.at == at) {
            return;
        }
        self.push_error(SyntaxError { kind, message, span, at, related: None });
    }

    fn push_error(&mut self, e: SyntaxError) {
        if !self.full() {
            self.errors.push(e);
        }
    }

    fn full(&self) -> bool {
        self.errors.len() >= self.max_errors
    }

    fn note(&mut self, kind: Neon3, span: Span, key: Option<u32>) {
        // Nested dash blocks decide their indentation on one line.
        let again = self.neon3.last().is_some_and(|n| n.kind == kind && n.span == span);
        if !again && self.neon3.len() < MAX_ERRORS {
            self.neon3.push(Neon3Note { kind, span, key });
        }
    }

    /// `on` and `off` as values (nette/neon 3.3 reads them as booleans).
    fn on_off(&mut self) {
        for (i, t) in self.toks.iter().enumerate() {
            if t.kind == TokenKind::Literal
                && t.span.end - t.span.start <= 3
                && self.roles[i] == Role::Value
                && matches!(t.span.text(self.text), "on" | "On" | "ON" | "off" | "Off" | "OFF")
            {
                self.note(Neon3::OnOff, t.span, None);
            }
        }
    }

    /// `Unexpected …` at what comes next, or `description` instead.
    fn unexpected_with(&mut self, kind: ErrorKind, description: Option<&str>) {
        if self.full() {
            return;
        }
        match self.peek() {
            Next::Newline => {
                let (at, count) = self.line_ending_at();
                let message = description
                    .map_or_else(|| format!("Unexpected '{}'", "<new line>".repeat(count.min(40))), str::to_owned);
                let span = if self.pos > 0 { self.span_of(self.pos - 1) } else { Span::new(at, at) };
                self.error(kind, message, span, at as u32);
            }
            Next::End => {
                let end = self.text.len();
                let message = description.unwrap_or("Unexpected end").to_owned();
                let span = if self.pos > 0 { self.span_of(self.pos - 1) } else { Span::new(end, end) };
                self.error(kind, message, span, end as u32);
            }
            Next::Token(TokenKind::Error) if description.is_none() => {} // reported by the scanner
            // Right after text the scanner couldn't read, on its line: no second problem.
            Next::Token(_)
                if description.is_none()
                    && self.pos > 0
                    && self.toks[self.pos - 1].kind == TokenKind::Error
                    && !self.toks[self.pos].newline => {}
            Next::Token(_) => {
                let span = self.span_of(self.pos);
                let message =
                    description.map_or_else(|| format!("Unexpected '{}'", quote(span.text(self.text))), str::to_owned);
                self.error(kind, message, span, span.start);
            }
        }
    }

    fn unexpected(&mut self) {
        self.unexpected_with(ErrorKind::Syntax, None);
    }

    /// The line ending before the next token, and how many follow it at once.
    fn line_ending_at(&self) -> (usize, usize) {
        let b = self.text.as_bytes();
        let mut i = if self.pos > 0 { self.span_of(self.pos - 1).end as usize } else { 0 };
        while i < b.len() && b[i] != b'\n' && !(b[i] == b'\r' && b.get(i + 1) == Some(&b'\n')) {
            i += 1;
        }
        let at = i;
        let mut count = 0;
        loop {
            match (b.get(i), b.get(i + 1)) {
                (Some(b'\n'), _) => i += 1,
                (Some(b'\r'), Some(b'\n')) => i += 2,
                _ => break,
            }
            count += 1;
        }
        (at, count.max(1))
    }

    /// Skips what is left of the line (brackets with their content).
    fn skip_line(&mut self) {
        while let Next::Token(k) = self.peek() {
            let i = self.bump();
            if k.is_open() {
                let pair = self.pairs[i];
                let to = if pair & CUT != 0 { (pair & !CUT) as usize } else { pair as usize + 1 };
                if to > self.pos {
                    self.pos = to.min(self.limit);
                }
            }
        }
    }

    /// One level deeper; false (and reported) beyond the limit.
    fn enter(&mut self, token: usize) -> bool {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            let span = self.span_of(token);
            self.error(ErrorKind::TooDeep, "Too deep nesting".into(), span, span.start);
            return false;
        }
        true
    }

    // ---- the document and blocks ----

    fn document(&mut self) -> (Option<Value>, Vec<Value>) {
        self.try_newline();
        let indent = self.indentation();
        let root = self.block(Pending::Known(Cow::Borrowed(indent)), false);
        let mut rest = Vec::new();
        self.try_newline();
        while self.is_any() {
            self.unexpected();
            let before = self.mark();
            let indent = self.indentation();
            if let Some(v) = self.block(Pending::Known(Cow::Borrowed(indent)), false) {
                rest.push(v);
            }
            if self.mark() == before {
                self.skip_line();
                if self.mark() == before {
                    self.bump();
                }
            }
            self.try_newline();
        }
        (root, rest)
    }

    fn block(&mut self, pending: Pending<'t>, only_bullets: bool) -> Option<Value> {
        let depth = self.depth;
        let v = self.block_items(&pending, only_bullets);
        self.depth = depth;
        v
    }

    /// The indentation a block expects, deciding a dash block's on first use.
    fn resolve(&mut self, pending: &Pending<'t>, next: &str) -> Cow<'t, str> {
        match pending {
            Pending::Known(s) => s.clone(),
            Pending::Dash(state) => {
                if let Some(r) = &state.borrow().resolved {
                    return Cow::Owned(r.to_string());
                }
                let base = state.borrow().base.clone();
                let base = self.resolve(&base, next);
                let tab = next.len() > base.len() && next.starts_with(&*base) && next.as_bytes()[base.len()] == b'\t';
                if tab {
                    let span = self.span_of(self.pos);
                    self.note(Neon3::TabAfterDash, span, None);
                }
                let resolved: Rc<str> = format!("{base}{}", if tab { "\t" } else { "  " }).into();
                let mut s = state.borrow_mut();
                s.resolved = Some(resolved.clone());
                s.line = self.pos;
                s.errors = self.errors.len();
                Cow::Owned(resolved.to_string())
            }
        }
    }

    /// Compares the next line's indentation with the block's: `Less`,
    /// `Equal` or `Greater`, reporting indentations that mix tabs and spaces.
    fn compare_indent(&mut self, pending: &Pending<'t>) -> std::cmp::Ordering {
        let next = self.indentation();
        let indent = self.resolve(pending, next);
        let n = next.len().min(indent.len());
        if next.as_bytes()[..n] != indent.as_bytes()[..n] {
            // Lines read after `Bad indentation` end at a line not like them:
            // the blocks around, which nette/neon knows, judge it.
            if self.recovering > 0 {
                return std::cmp::Ordering::Less;
            }
            self.unexpected_with(ErrorKind::TabsAndSpaces, Some("Invalid combination of tabs and spaces"));
        }
        next.len().cmp(&indent.len())
    }

    fn block_items(&mut self, pending: &Pending<'t>, only_bullets: bool) -> Option<Value> {
        let mut items: Vec<Item> = Vec::new();
        let mut keys = Keys::default();
        let start = self.pos;
        loop {
            let bullet = self.try_consume(&[TokenKind::Dash]);
            let mut key = None;
            let mut value = None;
            let mut broken = false;
            // A colon was read (the key itself may be unacceptable).
            let mut keyed = false;
            if let Some(b) = bullet {
                if items.is_empty() && !self.enter(b) {
                    self.skip_line();
                    return self.finish_block(items, start);
                }
            } else if !self.is_any() || only_bullets {
                return self.finish_block(items, start);
            } else {
                let item_start = self.pos;
                match self.value() {
                    None => {
                        self.skip_line();
                        broken = true;
                    }
                    Some(head) => {
                        if self.try_consume(&[TokenKind::Colon, TokenKind::Equals]).is_some() {
                            keyed = true;
                            key = self.add_key(&mut keys, &head);
                            if items.is_empty() && !self.enter(item_start) {
                                self.skip_line();
                                return self.finish_block(items, start);
                            }
                        } else if items.is_empty() {
                            return Some(head);
                        } else {
                            self.unexpected();
                            self.skip_line();
                            value = Some(head);
                            broken = true;
                        }
                    }
                }
            }

            if !broken {
                if self.try_newline() {
                    match self.compare_indent(pending) {
                        std::cmp::Ordering::Greater => {
                            let next = self.indentation();
                            value = self.block(Pending::Known(Cow::Borrowed(next)), false);
                        }
                        std::cmp::Ordering::Less => {
                            self.push_item(&mut items, &mut keys, bullet, key, None);
                            return self.finish_block(items, start);
                        }
                        std::cmp::Ordering::Equal => {
                            if keyed && self.is(TokenKind::Dash) {
                                let indent = self.resolve(pending, self.indentation());
                                value = self.block(Pending::Known(indent), true);
                            }
                        }
                    }
                } else if !keyed {
                    value = self.dash_block(pending);
                } else if self.is_any() {
                    value = self.value();
                    if value.is_none() || !matches!(self.peek(), Next::Newline | Next::End) {
                        if value.is_some() {
                            self.unexpected();
                        }
                        self.skip_line();
                    }
                }
            }
            self.push_item(&mut items, &mut keys, bullet, key, value);

            loop {
                self.try_newline();
                if !self.is_any() {
                    return self.finish_block(items, start);
                }
                match self.compare_indent(pending) {
                    std::cmp::Ordering::Less => return self.finish_block(items, start),
                    std::cmp::Ordering::Equal => break,
                    std::cmp::Ordering::Greater => {
                        self.unexpected_with(ErrorKind::BadIndentation, Some("Bad indentation"));
                        // Read the deeper lines as a block of their own, so they keep their structure.
                        let before = self.mark();
                        let next = self.indentation();
                        self.recovering += 1;
                        let orphan = self.block(Pending::Known(Cow::Borrowed(next)), false);
                        self.recovering -= 1;
                        if let Some(orphan) = orphan {
                            let span = self.value_span(&orphan);
                            items.push(Item { dash: None, key: None, value: Some(orphan), span });
                        }
                        if self.mark() == before {
                            self.skip_line();
                        }
                    }
                }
            }
        }
    }

    fn finish_block(&mut self, items: Vec<Item>, start: usize) -> Option<Value> {
        if items.is_empty() {
            return None;
        }
        let first = items.first().map_or(self.span_of(start), |i| i.span);
        let last = items.last().map_or(first, |i| i.span);
        Some(Value::Array(Array { kind: ArrayKind::Block, items, span: first.to(last), closed: true }))
    }

    /// A block opened on the line of a dash (§2.4): `- a: 1` + `  b: 2`.
    fn dash_block(&mut self, base: &Pending<'t>) -> Option<Value> {
        let state = Rc::new(RefCell::new(DashIndent { base: base.clone(), resolved: None, line: 0, errors: 0 }));
        let before = self.errors.len();
        let value = self.block(Pending::Dash(state.clone()), false);
        let s = state.borrow();
        // nette/neon tries a tab first and reports a failure inside such a
        // block as a mix of tabs and spaces where the tab was decided.
        if let Some(resolved) = &s.resolved
            && resolved.ends_with('\t')
            && self.errors.len() > before
            && s.errors == before
            && self.errors[before].kind != ErrorKind::TooDeep
        {
            let span = self.span_of(s.line);
            self.errors[before] = SyntaxError {
                kind: ErrorKind::TabsAndSpaces,
                message: "Invalid combination of tabs and spaces".into(),
                span,
                at: span.start,
                related: None,
            };
        }
        value
    }

    fn push_item(
        &mut self,
        items: &mut Vec<Item>,
        keys: &mut Keys<'t>,
        dash: Option<usize>,
        key: Option<usize>,
        value: Option<Value>,
    ) {
        let first = dash.or(key).map(|i| self.span_of(i));
        let value_span = value.as_ref().map(|v| self.value_span(v));
        let span = match (first, value_span) {
            (Some(a), Some(b)) => a.to(b),
            (Some(a), None) => {
                // The key with its colon.
                let colon = key.map_or(a, |k| self.span_of(k + 1));
                a.to(if colon.start >= a.end { colon } else { a })
            }
            (None, Some(b)) => b,
            (None, None) => return,
        };
        if key.is_none() {
            self.implicit_key(keys, value_span.unwrap_or(span));
        }
        items.push(Item { dash: dash.map(|d| d as u32), key: key.map(|k| k as u32), value, span });
    }

    fn implicit_key(&mut self, keys: &mut Keys<'t>, at: Span) {
        match keys.max {
            Some(i64::MAX) => {
                self.error(ErrorKind::ImplicitKeyOverflow, "Implicit key overflow".into(), at, at.start);
            }
            Some(m) => keys.max = Some(m + 1),
            None => keys.max = Some(0),
        }
    }

    /// Checks a key: a string or a literal, not repeated. The key token, if acceptable.
    fn add_key(&mut self, keys: &mut Keys<'t>, head: &Value) -> Option<usize> {
        let token = match head {
            Value::Scalar(i) => *i as usize,
            other => {
                let span = self.value_span(other);
                self.error(ErrorKind::UnacceptableKey, "Unacceptable key".into(), span, span.start);
                return None;
            }
        };
        self.roles[token] = Role::Key;
        let t = self.toks[token];
        let raw = t.span.text(self.text);
        if t.kind == TokenKind::Literal && scalar::is_float_tie(raw) {
            let bits = raw.parse::<f64>().map_or(0, f64::to_bits);
            match keys.ties.iter().find(|(b, _)| *b == bits) {
                Some(&(_, first)) => self.duplicate(raw, token, Some(first)),
                None => keys.ties.push((bits, token as u32)),
            }
            return Some(token);
        }
        let canonical: Cow<'t, str> = match t.kind {
            TokenKind::String => scalar::string_value(raw).unwrap_or(Cow::Borrowed(raw)),
            TokenKind::Literal => scalar::literal_key(raw),
            _ => return Some(token),
        };
        if let Some(k) = scalar::integer_key(&canonical) {
            keys.max = Some(keys.max.map_or(k, |m| m.max(k)));
        }
        let first = keys.small.iter().find(|(k, _)| *k == canonical).map(|(_, t)| *t);
        let seen = first.is_some() || keys.large.as_ref().is_some_and(|set| set.contains(&canonical));
        if seen {
            self.duplicate(&canonical, token, first);
        } else if let Some(set) = keys.large.as_mut() {
            set.insert(canonical);
        } else {
            keys.small.push((canonical, token as u32));
            if keys.small.len() > 32 {
                keys.large = Some(keys.small.iter().map(|(k, _)| k.clone()).collect());
            }
        }
        Some(token)
    }

    fn duplicate(&mut self, key: &str, token: usize, first: Option<u32>) {
        let span = self.span_of(token);
        let related = first.map(|f| (self.span_of(f as usize), "first defined here"));
        let message = format!("Duplicated key '{key}'");
        self.push_error(SyntaxError { kind: ErrorKind::DuplicateKey, message, span, at: span.start, related });
    }

    // ---- inline values ----

    /// A scalar or an inline array, then possibly an entity (§2.5). `None`
    /// (reported) when the next token can't start a value.
    fn value(&mut self) -> Option<Value> {
        let v = match self.peek() {
            Next::Token(TokenKind::String) => {
                let i = self.bump();
                self.check_string(i);
                Value::Scalar(i as u32)
            }
            Next::Token(TokenKind::Literal | TokenKind::Error) => Value::Scalar(self.bump() as u32),
            Next::Token(k) if k.is_open() => Value::Array(self.braces()),
            _ => {
                self.unexpected();
                return None;
            }
        };
        Some(if self.is(TokenKind::OpenParen) { self.entity(v) } else { v })
    }

    fn check_string(&mut self, i: usize) {
        let span = self.span_of(i);
        if let Err(e) = scalar::check_string(span.text(self.text)) {
            let kind = if e.hex { ErrorKind::HexEscape } else { ErrorKind::Escape };
            self.error(kind, e.message, span, span.start);
        }
    }

    fn entity(&mut self, head: Value) -> Value {
        self.entity_head(&head, Role::EntityHead);
        let attributes = self.braces();
        let span = self.value_span(&head).to(attributes.span);
        let mut links = vec![Entity { head, attributes: Some(attributes), span }];
        while let Some(t) = self.try_consume(&[TokenKind::Literal]) {
            let head = Value::Scalar(t as u32);
            self.entity_head(&head, Role::ChainLink);
            if self.is(TokenKind::OpenParen) {
                let attributes = self.braces();
                let span = self.span_of(t).to(attributes.span);
                links.push(Entity { head, attributes: Some(attributes), span });
            } else {
                links.push(Entity { head, attributes: None, span: self.span_of(t) });
                break;
            }
        }
        if links.len() == 1 { Value::Entity(Box::new(links.remove(0))) } else { Value::Chain(links) }
    }

    fn entity_head(&mut self, head: &Value, role: Role) {
        let Value::Scalar(i) = head else { return };
        let i = *i as usize;
        self.roles[i] = role;
        let t = self.toks[i];
        let raw = t.span.text(self.text);
        let name = match t.kind {
            TokenKind::String => scalar::string_value(raw).unwrap_or(Cow::Borrowed(raw)),
            _ => Cow::Borrowed(raw),
        };
        if name == "!!chain" {
            self.error(ErrorKind::ReservedEntity, "Entity name '!!chain' is reserved".into(), t.span, t.span.start);
        }
    }

    fn braces(&mut self) -> Array {
        let depth = self.depth;
        let a = self.braces_items();
        self.depth = depth;
        a
    }

    fn braces_items(&mut self) -> Array {
        let open = self.bump();
        let kind = match self.toks[open].kind {
            TokenKind::OpenBracket => ArrayKind::Bracket,
            TokenKind::OpenBrace => ArrayKind::Brace,
            _ => ArrayKind::Paren,
        };
        let pair = self.pairs[open];
        let (close, end) =
            if pair & CUT != 0 { (None, (pair & !CUT) as usize) } else { (Some(pair as usize), pair as usize) };
        let open_span = self.span_of(open);
        if !self.enter(open) {
            let to = close.map_or(end, |c| c + 1).min(self.limit).max(self.pos);
            self.pos = to;
            self.taken = false;
            let span = open_span.to(self.span_of(to.saturating_sub(1)));
            return Array { kind, items: Vec::new(), span, closed: close.is_some() };
        }
        let saved = self.limit;
        self.limit = self.limit.min(end);
        let mut items = Vec::new();
        let mut keys = Keys::default();
        let mut next: Option<(Value, usize)> = None;
        loop {
            let (head, colon) = match next.take() {
                Some((v, c)) => (v, Some(c)),
                None => {
                    self.try_newline();
                    if !self.is_any() {
                        break;
                    }
                    let Some(head) = self.value() else {
                        if let Next::Token(_) = self.peek() {
                            self.bump();
                        }
                        continue;
                    };
                    let colon = self.try_colon();
                    (head, colon)
                }
            };
            let mut key = None;
            let mut value = None;
            if colon.is_some() {
                key = self.add_key(&mut keys, &head);
                let line_end = self.try_newline();
                if !self.is(TokenKind::Comma) && self.is_any() {
                    match self.value() {
                        // A value after a line ending is the next key if a colon follows.
                        Some(v) => match if line_end { self.try_colon() } else { None } {
                            Some(c) => next = Some((v, c)),
                            None => {
                                if line_end && let Some(k) = key {
                                    let span = self.value_span(&v);
                                    self.note(Neon3::ValueOnNextLine, span, Some(k as u32));
                                }
                                value = Some(v);
                            }
                        },
                        None => {
                            if let Next::Token(_) = self.peek() {
                                self.bump();
                            }
                        }
                    }
                }
            } else {
                value = Some(head);
            }
            let before = self.mark();
            if next.is_none() {
                self.try_newline();
                if let Some(c) = self.try_consume(&[TokenKind::Comma])
                    && self.toks[c].newline
                {
                    self.note(Neon3::CommaOnNextLine, self.toks[c].span, None);
                }
            }
            self.push_item(&mut items, &mut keys, None, key, value);
            if next.is_none() && self.mark() == before && self.is_any() {
                self.unexpected();
                if !matches!(self.peek(), Next::Token(k) if k.is_open() || matches!(k, TokenKind::String | TokenKind::Literal | TokenKind::Error))
                    && let Next::Token(_) = self.peek()
                {
                    // A stray token, then maybe the comma meant to end the item.
                    self.bump();
                    self.try_consume(&[TokenKind::Comma]);
                }
            }
        }
        self.limit = saved;
        let closed = close.is_some_and(|c| c == self.pos);
        let span = if closed {
            let c = self.bump();
            open_span.to(self.span_of(c))
        } else {
            // The line ending before the cut belongs to the block around.
            self.taken = false;
            let last = items.last().map_or(open_span, |i: &Item| i.span);
            let at = if end < self.toks.len() { self.span_of(end).start } else { self.text.len() as u32 };
            let closer = match kind {
                ArrayKind::Bracket => ']',
                ArrayKind::Brace => '}',
                _ => ')',
            };
            let message = if end < self.toks.len() {
                format!("Unclosed '{}': missing '{closer}'", open_span.text(self.text))
            } else {
                format!("Unexpected end: '{}' is not closed", open_span.text(self.text))
            };
            self.push_error(SyntaxError { kind: ErrorKind::Unclosed, message, span: open_span, at, related: None });
            open_span.to(last)
        };
        Array { kind, items, span, closed }
    }

    /// The colon or equals sign after a key, possibly on a later line.
    fn try_colon(&mut self) -> Option<usize> {
        let mark = self.mark();
        self.try_newline();
        let colon = self.try_consume(&[TokenKind::Colon, TokenKind::Equals]);
        match colon {
            Some(c) if self.toks[c].newline => self.note(Neon3::ColonOnNextLine, self.toks[c].span, None),
            Some(_) => {}
            None => self.reset(mark),
        }
        colon
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neon::lex;

    fn errors(text: &str) -> Vec<String> {
        let lexed = lex(text);
        let doc = parse(text, &lexed);
        let lines = crate::lines::LineIndex::new(text);
        doc.errors
            .iter()
            .map(|e| {
                let p = lines.position(text, e.at as usize);
                // nette/neon counts columns in code points, 1-based.
                let col = text[lines.line_start(p.line) as usize..e.at as usize].chars().count() + 1;
                format!("{} on line {} at column {}", e.message, p.line + 1, col)
            })
            .collect()
    }

    fn first_error(text: &str) -> String {
        errors(text).into_iter().next().unwrap_or_else(|| "accepted".into())
    }

    /// The document as compact JSON-like text: keys, values and nesting.
    fn dump(text: &str) -> String {
        let lexed = lex(text);
        let doc = parse(text, &lexed);
        assert!(doc.errors.is_empty(), "{text:?}: {:?}", doc.errors);
        fn value(v: &Value, text: &str, toks: &[Token], out: &mut String) {
            match v {
                Value::Scalar(i) => out.push_str(toks[*i as usize].span.text(text)),
                Value::Array(a) => {
                    out.push('{');
                    items(a, text, toks, out);
                    out.push('}');
                }
                Value::Entity(e) => link(e, text, toks, out),
                Value::Chain(links) => links.iter().for_each(|l| link(l, text, toks, out)),
            }
        }
        fn link(e: &Entity, text: &str, toks: &[Token], out: &mut String) {
            value(&e.head, text, toks, out);
            if let Some(a) = &e.attributes {
                out.push('(');
                items(a, text, toks, out);
                out.push(')');
            }
        }
        fn items(a: &Array, text: &str, toks: &[Token], out: &mut String) {
            for (n, item) in a.items.iter().enumerate() {
                if n > 0 {
                    out.push_str(", ");
                }
                if let Some(k) = item.key {
                    out.push_str(toks[k as usize].span.text(text));
                    out.push_str(": ");
                }
                match &item.value {
                    Some(v) => value(v, text, toks, out),
                    None if a.kind == ArrayKind::Paren => {}
                    None => out.push_str("null"),
                }
            }
        }
        let mut out = String::new();
        if let Some(root) = &doc.root {
            value(root, text, &lexed.tokens, &mut out);
        }
        out
    }

    #[test]
    fn blocks_by_indentation() {
        assert_eq!(dump("a: 1\nb:\n\tc: 2\n\td:\n\t\t- x\n\t\t- y\ne: 3"), "{a: 1, b: {c: 2, d: {x, y}}, e: 3}");
        assert_eq!(dump("pets:\n   - Cat\n   - Dog\ncars:\n   - Volvo"), "{pets: {Cat, Dog}, cars: {Volvo}}");
        assert_eq!(
            dump("- name: John\n  age: 35\n- name: Peter\n  age: 28"),
            "{{name: John, age: 35}, {name: Peter, age: 28}}"
        );
        assert_eq!(dump("-\n\tname: John\n-\n\tname: Peter"), "{{name: John}, {name: Peter}}");
        assert_eq!(dump("- - c"), "{{c}}");
        assert_eq!(dump("a:\n- x\n- y\nb: 1"), "{a: {x, y}, b: 1}");
        assert_eq!(
            dump("- Cat\nstreet: 742 Evergreen Terrace\n- Goldfish"),
            "{Cat, street: 742 Evergreen Terrace, Goldfish}"
        );
        assert_eq!(dump("a:\n  x"), "{a: x}");
        assert_eq!(
            dump("search:\n\t-\tin: %appDir%\n\t\tclasses:\n\t\t\t- *Factory"),
            "{search: {{in: %appDir%, classes: {*Factory}}}}"
        );
        assert_eq!(dump("# comment\n\n  a: 1 # c\n  b: 2\n"), "{a: 1, b: 2}");
        assert_eq!(dump("abc"), "abc");
        assert_eq!(dump(""), "");
    }

    #[test]
    fn inline_arrays_and_entities() {
        assert_eq!(dump("{a: b, c: [1, 2], d}"), "{a: b, c: {1, 2}, d}");
        assert_eq!(dump("{a= b, c=d}"), "{a: b, c: d}");
        assert_eq!(dump("x: [\n\tVolvo,\n\tSkoda,\n]"), "{x: {Volvo, Skoda}}");
        assert_eq!(dump("{\n\tstreet:\n\t\t742 Evergreen\n}"), "{street: 742 Evergreen}");
        assert_eq!(dump("{a:\nb: 1}"), "{a: null, b: 1}");
        assert_eq!(dump("{g:, h:}"), "{g: null, h: null}");
        assert_eq!(dump("Column(type: int, nulls: yes)"), "Column(type: int, nulls: yes)");
        assert_eq!(dump("a: Foo(1)::bar(2)::baz"), "{a: Foo(1)::bar(2)::baz}");
        assert_eq!(dump("x: @foo::bar(%a%, 'x')"), "{x: @foo::bar(%a%, 'x')}");
        assert_eq!(dump("PDO(\n\tusername: root\n\tdsn: 'x'\n)"), "PDO(username: root, dsn: 'x')");
        assert_eq!(
            dump("level: schema(anyOf(int(), string()), nullable())"),
            "{level: schema(anyOf(int(), string()), nullable())}"
        );
        assert_eq!(dump("a: structure([\n\tx: listOf(string()),\n])\n"), "{a: structure({x: listOf(string())})}");
    }

    #[test]
    fn errors_like_nette() {
        for (text, message) in [
            ("Hello\nWorld", "Unexpected 'World' on line 2 at column 1"),
            ("- Dave,\n- Rimmer,\n- Kryten,\n", "Unexpected ',' on line 1 at column 7"),
            ("item [a, b]", "Unexpected ',' on line 1 at column 8"),
            ("{,}", "Unexpected ',' on line 1 at column 2"),
            ("{a, ,}", "Unexpected ',' on line 1 at column 5"),
            ("\"", "Unexpected '\"' on line 1 at column 1"),
            ("\ta:\n b:", "Invalid combination of tabs and spaces on line 2 at column 2"),
            ("- x: 20\n  - a: 10\n\tb: 10", "Invalid combination of tabs and spaces on line 3 at column 2"),
            ("a:\n  b:\n c: x\n", "Bad indentation on line 3 at column 2"),
            ("a: 1\n  b:", "Bad indentation on line 2 at column 3"),
            ("- x:\n a: 10", "Bad indentation on line 2 at column 2"),
            ("- x: 20\n   a: 10", "Bad indentation on line 2 at column 4"),
            ("- x: 20\n a: 10", "Bad indentation on line 2 at column 2"),
            ("- a: 1\n\t\tb: 2", "Invalid combination of tabs and spaces on line 2 at column 3"),
            ("- a: 1\n\tb: 2\n\tc", "Invalid combination of tabs and spaces on line 2 at column 2"),
            ("- a: 1\n    b: 2", "Bad indentation on line 2 at column 5"),
            ("- x: y:", "Unexpected ':' on line 1 at column 7"),
            ("\nfoo:\nbar\n", "Unexpected '<new line>' on line 3 at column 4"),
            ("a: 1\nfoo\n\nbar", "Unexpected '<new line><new line>' on line 2 at column 4"),
            ("a: 1\na: 2", "Duplicated key 'a' on line 2 at column 1"),
            ("{1.0: a, 1: b}", "Duplicated key '1' on line 1 at column 10"),
            ("{9223372036854775807: a, b}", "Implicit key overflow on line 1 at column 26"),
            ("{ []: foo }", "Unacceptable key on line 1 at column 3"),
            ("[]: foo", "Unacceptable key on line 1 at column 1"),
            ("{ - 1}", "Unexpected '-' on line 1 at column 3"),
            ("žluťoučký: ]", "Unexpected ']' on line 1 at column 12"),
            ("žluť: '''\n\tžluťoučký\n''' ]", "Unexpected ']' on line 3 at column 5"),
            ("\"\\a\"", "Invalid escaping sequence \\a on line 1 at column 1"),
            ("x: !!chain(a)", "Entity name '!!chain' is reserved on line 1 at column 4"),
            ("x: '!!chain'(a)", "Entity name '!!chain' is reserved on line 1 at column 4"),
            ("[1, 2", "Unexpected end: '[' is not closed on line 1 at column 6"),
            // A colon or equals sign after a key's value inside brackets.
            ("x: {a: b:, c: d}", "Unexpected ':' on line 1 at column 9"),
            ("x: [a: b:]", "Unexpected ':' on line 1 at column 9"),
            ("x: Foo(x: y=)", "Unexpected '=' on line 1 at column 12"),
            ("{a: b: c}", "Unexpected ':' on line 1 at column 6"),
            ("{a: 1 b:}", "Unexpected ':' on line 1 at column 8"),
            ("x: {\n\ta: 1\n: }", "Unexpected ':' on line 3 at column 1"),
            (
                "services:\n\t- Foo(\n\t\thost: localhost\n\t\tport: 3306:\n\t)\n",
                "Unexpected ':' on line 4 at column 13",
            ),
        ] {
            assert_eq!(first_error(text), message, "{text:?}");
        }
    }

    #[test]
    fn one_mistake_is_one_problem() {
        for (text, message) in [
            ("x: {a: b:, c: d}\n", "Unexpected ':' on line 1 at column 9"),
            // The line after a badly indented one is judged by the block around it.
            (
                "services:\n\t- A\n    - B\n\t- C\n\t- D\n",
                "Invalid combination of tabs and spaces on line 3 at column 5",
            ),
            ("a:\n\tb: 1\n  c: 2\n\td: 3\n", "Invalid combination of tabs and spaces on line 3 at column 3"),
        ] {
            assert_eq!(errors(text), [message], "{text:?}");
        }
        // A real mix after the bad line is still reported.
        let found = errors("a:\n\tb: 1\n\t\t\tc: 2\n  d: 3\n");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[1].starts_with("Invalid combination of tabs and spaces on line 4"), "{found:?}");
    }

    #[test]
    fn float_keys_nette_neon_versions_round_differently_are_duplicates_of_the_same_number_only() {
        // nette/neon 3 reads 12345678901234.5 as 12345678901234, 4.0.1 as 12345678901235.
        assert_eq!(first_error("{12345678901234.5: a, 12345678901234: b}"), "accepted");
        assert_eq!(first_error("{12345678901234.5: a, 12345678901235: b}"), "accepted");
        assert_eq!(
            first_error("{12345678901234.5: a, 12345678901234.50: b}"),
            "Duplicated key '12345678901234.50' on line 1 at column 23"
        );
        // 4.0.1 refuses this one, 3.x reads 1234567890123.4 and accepts it.
        assert_eq!(first_error("{1234567890123.45: a, 1234567890123.5: b}"), "accepted");
    }

    #[test]
    fn what_nette_neon_3_reads_otherwise_is_noted() {
        fn notes(text: &str) -> Vec<(Neon3, &str)> {
            let lexed = lex(text);
            let doc = parse(text, &lexed);
            assert!(doc.errors.is_empty(), "{text:?}: {:?}", doc.errors);
            doc.neon3.iter().map(|n| (n.kind, &text[n.span.range()])).collect()
        }
        assert_eq!(notes("x: {a:\n\tb}"), [(Neon3::ValueOnNextLine, "b")]);
        assert_eq!(notes("x: (a:\n\tFoo(1))"), [(Neon3::ValueOnNextLine, "Foo(1)")]);
        assert_eq!(notes("x: {a:\n\tb: c}"), [], "the next key");
        assert_eq!(notes("x: {a\n= b}"), [(Neon3::ColonOnNextLine, "=")]);
        assert_eq!(notes("x: [a\n\t, b]"), [(Neon3::CommaOnNextLine, ",")]);
        assert_eq!(notes("x: {a:\n, b}"), [(Neon3::CommaOnNextLine, ",")]);
        assert_eq!(notes("x: [a,\n\tb]"), []);
        assert_eq!(notes("search:\n\t-\tin: %appDir%\n\t\tclasses: x"), [(Neon3::TabAfterDash, "classes")]);
        assert_eq!(notes("- - a\n\t- b"), [(Neon3::TabAfterDash, "-")], "once for nested dashes");
        assert_eq!(notes("- a: 1\n  b: 2\n- a: 1\n"), []);
        assert_eq!(
            notes("a: off\nb: [ON, Off, 'off', offline]\noff: x"),
            [(Neon3::OnOff, "off"), (Neon3::OnOff, "ON"), (Neon3::OnOff, "Off")]
        );
        assert_eq!(notes("\u{feff}a: 1"), [(Neon3::ByteOrderMark, "\u{feff}")]);
    }

    #[test]
    fn problems_are_kept_up_to_a_limit() {
        let text = "x\n".repeat(5000);
        let doc = parse(&text, &lex(&text));
        assert_eq!(doc.errors.len(), MAX_ERRORS);
        let text = "` ".repeat(5000);
        let lexed = lex(&text);
        let doc = parse(&text, &lexed);
        assert_eq!(lexed.errors.len(), MAX_ERRORS);
        assert!(doc.errors.len() <= 2 * MAX_ERRORS);
    }

    #[test]
    fn brackets_pair_in_linear_time() {
        let n = 20_000;
        let text = format!("{}{}", "[".repeat(n), ")".repeat(n));
        let lexed = lex(&text);
        assert!(pairs(&lexed.tokens)[..n].iter().all(|&p| p & CUT != 0), "nothing closes");
        let text = "a: [(]) [x)]";
        let lexed = lex(text);
        let toks: Vec<&str> = lexed.tokens.iter().map(|t| t.span.text(text)).collect();
        let p = pairs(&lexed.tokens);
        assert_eq!((toks[2], p[2]), ("[", 4), "`]` closes `[` and the `(` inside");
        assert_eq!(p[3] & CUT, CUT);
        assert_eq!((toks[6], p[6]), ("[", 9), "`)` with no `(` open is skipped");
    }

    #[test]
    fn nesting_is_limited_without_overflowing_the_stack() {
        let ok = format!("{}{}", "[".repeat(500), "]".repeat(500));
        assert_eq!(first_error(&ok), "accepted");
        let deep = format!("{}{}", "[".repeat(501), "]".repeat(501));
        assert_eq!(first_error(&deep), "Too deep nesting on line 1 at column 501");
        assert_eq!(first_error(&format!("a: {}", "[".repeat(1000))), "Too deep nesting on line 1 at column 503");
        let dashes = "- ".repeat(600);
        assert!(first_error(&dashes).starts_with("Too deep nesting"));
    }

    #[test]
    fn a_problem_leaves_the_rest_of_the_tree() {
        let text = "parameters:\n\tfoo: [1, 2\n\tbar: 3\nservices:\n\t- Foo\n";
        let lexed = lex(text);
        let doc = parse(text, &lexed);
        assert_eq!(doc.errors.len(), 1, "{:?}", doc.errors);
        assert_eq!(doc.errors[0].kind, ErrorKind::Unclosed);
        let root = doc.root.as_ref().and_then(Value::as_array).unwrap();
        let keys: Vec<&str> =
            root.items.iter().filter_map(|i| i.key).map(|k| lexed.tokens[k as usize].span.text(text)).collect();
        assert_eq!(keys, ["parameters", "services"]);
        let params = root.items[0].value.as_ref().and_then(Value::as_array).unwrap();
        assert_eq!(params.items.len(), 2, "bar stays a parameter");

        let text = "a: 1\n  b: 2\nc: 3 x, y\nd: \"open\ne: 5";
        let lexed = lex(text);
        let doc = parse(text, &lexed);
        let kinds: Vec<ErrorKind> = doc.errors.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [ErrorKind::Syntax, ErrorKind::BadIndentation, ErrorKind::Syntax]);
        let root = doc.root.as_ref().and_then(Value::as_array).unwrap();
        let keys: Vec<&str> =
            root.items.iter().filter_map(|i| i.key).map(|k| lexed.tokens[k as usize].span.text(text)).collect();
        assert_eq!(keys, ["a", "c", "d", "e"]);
    }

    #[test]
    fn roles_mark_keys_and_entities() {
        let text = "a: Foo(b: 1)::bar()";
        let lexed = lex(text);
        let doc = parse(text, &lexed);
        let roles: Vec<(&str, Role)> =
            lexed.tokens.iter().zip(&doc.roles).map(|(t, r)| (t.span.text(text), *r)).collect();
        assert_eq!(roles[0], ("a", Role::Key));
        assert_eq!(roles[2], ("Foo", Role::EntityHead));
        assert_eq!(roles[4], ("b", Role::Key));
        assert_eq!(roles[8], ("::bar", Role::ChainLink));
    }
}
