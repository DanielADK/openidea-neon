//! Highlighting: each token by its role in the tree, literals split into
//! the parts of Nette's expression language (`@service::method`,
//! `Class::CONSTANT`, `%parameter%`), escapes inside strings.

use crate::neon::scalar::{self, LiteralKind};
use crate::neon::{Parsed, Role, TokenKind};
use openidea_plugin::syntax::{Highlight, Token};

/// The host keeps at most this many tokens per document.
pub const MAX_TOKENS: usize = 200_000;
/// Larger documents (baselines) get no punctuation tokens, to stay under [`MAX_TOKENS`].
const PUNCTUATION_LIMIT: usize = 1 << 20;

pub fn highlights(p: &Parsed) -> Vec<Token> {
    let mut out = Out { tokens: Vec::with_capacity(p.lexed.tokens.len() + p.lexed.comments.len()) };
    let punctuation = p.text.len() <= PUNCTUATION_LIMIT;
    let mut comments = p.lexed.comments.iter().peekable();
    for (i, t) in p.lexed.tokens.iter().enumerate() {
        while let Some(c) = comments.next_if(|c| c.start < t.span.start) {
            out.push(c.start as usize, c.end as usize, Highlight::Comment);
        }
        if out.tokens.len() >= MAX_TOKENS {
            break;
        }
        let (start, end) = (t.span.start as usize, t.span.end as usize);
        let text = t.span.text(&p.text);
        let role = p.doc.role(i);
        match t.kind {
            TokenKind::Literal => literal(&mut out, text, start, role),
            TokenKind::String if role == Role::Key => out.push(start, end, Highlight::Property),
            TokenKind::String => string(&mut out, text, start),
            TokenKind::Error if text.starts_with(['\'', '"']) => out.push(start, end, Highlight::String),
            TokenKind::Error => {}
            _ if punctuation => out.push(start, end, Highlight::Punctuation),
            _ => {}
        }
    }
    for c in comments {
        out.push(c.start as usize, c.end as usize, Highlight::Comment);
    }
    out.tokens.truncate(MAX_TOKENS);
    out.tokens
}

struct Out {
    tokens: Vec<Token>,
}

impl Out {
    fn push(&mut self, start: usize, end: usize, kind: Highlight) {
        if end > start {
            self.tokens.push(Token { range: start..end, kind });
        }
    }
}

fn literal(out: &mut Out, text: &str, base: usize, role: Role) {
    let end = base + text.len();
    match role {
        Role::Key => out.push(base, end, Highlight::Property),
        Role::Value => match scalar::classify(text) {
            LiteralKind::Null | LiteralKind::Bool => out.push(base, end, Highlight::Keyword),
            LiteralKind::Int | LiteralKind::Float | LiteralKind::Date => out.push(base, end, Highlight::Number),
            LiteralKind::Text => expression(out, text, base, role),
        },
        Role::EntityHead | Role::ChainLink => expression(out, text, base, role),
    }
}

/// A PHP-like name: `Foo`, `App\Model\Foo`, `\PDO`.
pub fn is_class_name(s: &str) -> bool {
    let s = s.strip_prefix('\\').unwrap_or(s);
    !s.is_empty()
        && s.split('\\').all(|part| {
            let mut chars = part.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii())
        })
}

fn starts_upper(s: &str) -> bool {
    s.trim_start_matches('\\').chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// A literal of Nette's expression language.
fn expression(out: &mut Out, text: &str, base: usize, role: Role) {
    if text.contains('%') {
        parameters(out, text, base, Highlight::String);
        return;
    }
    let (head, members) = match text.find("::") {
        Some(k) => (&text[..k], Some(k)),
        None => (text, None),
    };
    let head_end = base + head.len();
    if let Some(name) = head.strip_prefix('@') {
        if !name.is_empty() {
            out.push(base, head_end, Highlight::Macro);
        }
    } else if !head.is_empty() {
        let kind = if is_class_name(head) && (head.contains('\\') || starts_upper(head) || members.is_some()) {
            Highlight::Type
        } else if members.is_some() {
            Highlight::String
        } else {
            match role {
                Role::EntityHead => Highlight::Function,
                Role::ChainLink => Highlight::FunctionCall,
                _ => Highlight::String,
            }
        };
        out.push(base, head_end, kind);
    }
    let Some(mut at) = members else { return };
    let calls = role != Role::Value;
    // `::member` parts: methods (lowercase) and constants (uppercase).
    while at < text.len() {
        let rest = &text[at..];
        if !rest.starts_with("::") {
            out.push(base + at, base + text.len(), Highlight::String);
            return;
        }
        out.push(base + at, base + at + 2, Highlight::Operator);
        let name_start = at + 2;
        let name_end = text[name_start..].find("::").map_or(text.len(), |k| name_start + k);
        let name = &text[name_start..name_end];
        let last = name_end == text.len();
        let kind = if starts_upper(name) && !(calls && last) || name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        {
            Highlight::Constant
        } else {
            Highlight::FunctionCall
        };
        out.push(base + name_start, base + name_end, kind);
        at = name_end;
    }
}

/// `%name%` references (and `%%`) inside other text.
fn parameters(out: &mut Out, text: &str, base: usize, rest: Highlight) {
    let mut plain = 0;
    for (start, end, escape) in percent_parts(text) {
        out.push(base + plain, base + start, rest);
        out.push(base + start, base + end, if escape { Highlight::Escape } else { Highlight::Label });
        plain = end;
    }
    out.push(base + plain, base + text.len(), rest);
}

/// The `%name%` references (`false`) and `%%` escapes (`true`) of a text, as
/// Nette DI reads them.
pub fn percent_parts(text: &str) -> Vec<(usize, usize, bool)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(k) = text[i..].find('%') {
        let start = i + k;
        let mut j = start + 1;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'.' | b'-') || b[j] >= 0x80) {
            j += 1;
        }
        if j < b.len() && b[j] == b'%' {
            out.push((start, j + 1, j == start + 1));
            i = j + 1;
        } else {
            i = start + 1;
        }
    }
    out
}

fn string(out: &mut Out, text: &str, base: usize) {
    let b = text.as_bytes();
    let double = b.first() == Some(&b'"');
    let multiline = (text.starts_with("'''") || text.starts_with("\"\"\"")) && matches!(b.get(3), Some(b'\n' | b'\r'));
    // Escapes: `\x` in double quotes, `''` in single-line single quotes.
    let (from, to) = if multiline { (3, text.len().saturating_sub(3)) } else { (1, text.len().saturating_sub(1)) };
    let mut plain = 0;
    let mut i = from;
    while i < to {
        let len = match b[i] {
            b'\\' if double && i + 1 < to && b[i + 1] != b'\n' && b[i + 1] != b'\r' => {
                if b[i + 1] == b'u' && i + 6 <= to && b[i + 2..i + 6].iter().all(u8::is_ascii_hexdigit) {
                    6
                } else {
                    1 + text[i + 1..].chars().next().map_or(0, char::len_utf8)
                }
            }
            b'\'' if !double && !multiline && i + 1 < to && b[i + 1] == b'\'' => 2,
            _ => {
                i += 1;
                continue;
            }
        };
        parameters(out, &text[plain..i], base + plain, Highlight::String);
        out.push(base + i, base + i + len, Highlight::Escape);
        i += len;
        plain = i;
    }
    parameters(out, &text[plain..], base + plain, Highlight::String);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(String, &'static str)> {
        let p = Parsed::new(text.to_owned());
        highlights(&p).into_iter().map(|t| (text[t.range].to_owned(), t.kind.name())).collect()
    }

    fn has(text: &str, part: &str, kind: &str) -> bool {
        kinds(text).iter().any(|(t, k)| t == part && *k == kind)
    }

    #[test]
    fn keys_values_and_comments() {
        let k = kinds("# c\nport: 587 # p\nname: Foo bar\non: yes\nwhen: 2016-06-03");
        assert_eq!(k[0], ("# c".into(), "comment"));
        assert_eq!(k[1], ("port".into(), "property"));
        assert_eq!(k[2], (":".into(), "punctuation"));
        assert_eq!(k[3], ("587".into(), "number"));
        assert_eq!(k[4], ("# p".into(), "comment"));
        assert!(has("name: Foo bar", "Foo bar", "string"));
        assert!(has("on: yes", "yes", "keyword"));
        assert!(has("x: on", "on", "string"), "on is a string in NEON");
        assert!(has("when: 2016-06-03", "2016-06-03", "number"));
        assert!(has("'quoted key': 1", "'quoted key'", "property"));
    }

    #[test]
    fn nette_expressions() {
        assert!(has("- App\\Model\\Foo(@db)", "App\\Model\\Foo", "type"));
        assert!(has("- App\\Model\\Foo(@db)", "@db", "macro"));
        assert!(has("a: not(%debugMode%)", "not", "function"));
        assert!(has("a: not(%debugMode%)", "%debugMode%", "label"));
        let k = kinds("a: @http.request::getUrl()::getHost()");
        assert!(k.contains(&("@http.request".into(), "macro")));
        assert!(k.contains(&("getUrl".into(), "function-call")));
        assert!(k.contains(&("::".into(), "operator")));
        assert!(k.contains(&("getHost".into(), "function-call")));
        assert!(has("- setAttribute(PDO::ATTR_ERRMODE)", "ATTR_ERRMODE", "constant"));
        assert!(has("- setAttribute(PDO::ATTR_ERRMODE)", "PDO", "type"));
        assert!(has("a: @settings::Version", "Version", "constant"));
        assert!(has("- App\\Core\\RouterFactory::createRouter", "createRouter", "function-call"));
        assert!(has("a: ::getenv(DB)", "getenv", "function-call"));
        assert!(has("dir: %appDir%/Forms", "/Forms", "string"));
        assert!(has("dir: %appDir%/Forms", "%appDir%", "label"));
    }

    #[test]
    fn strings_with_escapes_and_parameters() {
        let k = kinds(r#"a: "x\ty \u00A9 %tempDir%/c""#);
        assert!(k.contains(&(r"\t".into(), "escape")));
        assert!(k.contains(&(r"\u00A9".into(), "escape")));
        assert!(k.contains(&("%tempDir%".into(), "label")));
        assert!(k.contains(&("/c\"".into(), "string")));
        assert!(has("a: 'it''s 100%%'", "''", "escape"));
        assert!(has("a: 'it''s 100%%'", "%%", "escape"));
    }

    #[test]
    fn tokens_do_not_overlap_and_follow_the_text() {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/services.neon")).unwrap();
        let p = Parsed::new(text.clone());
        let mut end = 0;
        for t in highlights(&p) {
            assert!(t.range.start >= end && t.range.end > t.range.start, "{:?}", t.range);
            assert!(text.is_char_boundary(t.range.start) && text.is_char_boundary(t.range.end));
            end = t.range.end;
        }
    }
}
