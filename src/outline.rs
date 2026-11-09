//! The structure view (nested keys; services by name or class) and folding
//! (indented blocks, multi-line brackets, multiline strings, comment runs).

use crate::neon::{Array, ArrayKind, Item, Parsed, TokenKind, Value};
use openidea_plugin::syntax::{Fold, FoldKind, Symbol, SymbolKind};

/// Symbols of one array shown before the rest is summed up in one entry.
const MAX_CHILDREN: usize = 500;
/// Symbols of a whole document.
const MAX_SYMBOLS: usize = 10_000;
const MAX_NAME: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Root,
    /// Items of the top-level `services` section.
    Services,
    Nested,
}

pub fn outline(p: &Parsed) -> Vec<Symbol> {
    let mut out = Vec::new();
    for v in p.doc.values() {
        if let Value::Array(a) = v {
            walk(p, a, None, Place::Root, &mut out);
        }
    }
    out
}

fn walk(p: &Parsed, array: &Array, parent: Option<usize>, place: Place, out: &mut Vec<Symbol>) {
    for (n, item) in array.items.iter().enumerate() {
        if out.len() >= MAX_SYMBOLS {
            return;
        }
        if n == MAX_CHILDREN {
            let rest = &array.items[n..];
            let span = rest[0].span.to(rest[rest.len() - 1].span);
            out.push(Symbol {
                name: format!("… {} more", rest.len()),
                detail: None,
                kind: SymbolKind::Key,
                range: span.range(),
                name_range: rest[0].span.start as usize..rest[0].span.start as usize,
                parent,
            });
            return;
        }
        let (name, name_range) = item_name(p, item);
        let children =
            item.value.as_ref().and_then(Value::as_array).filter(|a| {
                !a.items.is_empty() && (a.kind == ArrayKind::Block || a.items.iter().any(|i| i.key.is_some()))
            });
        let kind = match place {
            Place::Root if item.key.is_some() => SymbolKind::Module,
            Place::Services => SymbolKind::Class,
            _ if item.key.is_some() && children.is_some() => SymbolKind::Key,
            _ if item.key.is_some() => SymbolKind::Property,
            _ if children.is_some() => SymbolKind::Struct,
            _ => SymbolKind::EnumMember,
        };
        let detail = if place == Place::Services { service_class(p, item) } else { value_detail(p, item) };
        let detail = detail.filter(|d| *d != name);
        let index = out.len();
        out.push(Symbol { name, detail, kind, range: item.span.range(), name_range, parent });
        if let Some(children) = children {
            let next = if place == Place::Root && item.key.is_some_and(|k| p.key_name(k) == "services") {
                Place::Services
            } else {
                Place::Nested
            };
            walk(p, children, Some(index), next, out);
        }
    }
}

/// The first line of `s`, at most [`MAX_NAME`] characters.
fn clip(s: &str) -> String {
    let s = s.trim();
    let line = s.lines().next().unwrap_or("").trim_end();
    match line.char_indices().nth(MAX_NAME) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None if line.len() < s.len() => format!("{line}…"),
        None => line.to_owned(),
    }
}

/// A scalar as a name: its value, or as written when that is blank (`''`).
fn scalar_name(p: &Parsed, token: u32) -> String {
    let name = clip(&p.scalar_text(token));
    if name.is_empty() { clip(p.token_text(token)) } else { name }
}

/// A one-line summary of a value.
fn summary(p: &Parsed, value: &Value) -> Option<String> {
    match value {
        Value::Scalar(t) => Some(scalar_name(p, *t)),
        Value::Entity(e) => Some(format!("{}(…)", summary(p, &e.head)?)),
        Value::Chain(links) => Some(format!("{}(…)…", summary(p, &links.first()?.head)?)),
        Value::Array(a) => {
            let first = a.items.first()?;
            match (first.key, &first.value) {
                (Some(_), Some(v)) => summary(p, v),
                (None, Some(v)) if a.kind != ArrayKind::Block => Some(format!("[{}, …]", summary(p, v)?)),
                _ => None,
            }
        }
    }
}

fn item_name(p: &Parsed, item: &Item) -> (String, std::ops::Range<usize>) {
    if let Some(k) = item.key {
        let span = p.lexed.tokens[k as usize].span;
        return (scalar_name(p, k), span.range());
    }
    let start = item.dash.map_or(item.span, |d| p.lexed.tokens[d as usize].span);
    let name = item.value.as_ref().and_then(|v| match v {
        // A map in a list (an `ignoreErrors` entry): named by its first value.
        Value::Array(a) if a.kind == ArrayKind::Block => {
            summary(p, v).or_else(|| Some(format!("{} items", a.items.len())))
        }
        Value::Entity(e) => summary(p, &e.head),
        _ => summary(p, v),
    });
    (name.unwrap_or_else(|| "-".into()), start.range())
}

fn value_detail(p: &Parsed, item: &Item) -> Option<String> {
    match (&item.key, item.value.as_ref()?) {
        (Some(_), v @ (Value::Scalar(_) | Value::Entity(_) | Value::Chain(_))) => summary(p, v),
        (Some(_), v @ Value::Array(a)) if a.kind != ArrayKind::Block => summary(p, v),
        (None, Value::Array(a)) => a.items.first().and_then(|i| i.key).map(|k| clip(&p.key_name(k))),
        _ => None,
    }
}

/// What a service is created from: its class or factory.
pub fn service_class(p: &Parsed, item: &Item) -> Option<String> {
    match item.value.as_ref()? {
        Value::Scalar(t) => Some(scalar_name(p, *t)),
        Value::Entity(e) => summary(p, &e.head),
        Value::Chain(links) => summary(p, &links.first()?.head),
        Value::Array(a) => ["create", "factory", "class", "type", "implement"].iter().find_map(|name| {
            let entry = a.items.iter().find(|i| i.key.is_some_and(|k| p.key_name(k) == *name))?;
            match entry.value.as_ref()? {
                Value::Entity(e) => summary(p, &e.head),
                Value::Chain(links) => summary(p, &links.first()?.head),
                v => summary(p, v),
            }
        }),
    }
}

pub fn folding(p: &Parsed) -> Vec<Fold> {
    let mut out = Vec::new();
    let mut fold = |start: u32, end: u32, kind: FoldKind| {
        let (a, b) = (p.line(start), p.line(end));
        if b > a {
            out.push(Fold { start_line: a, end_line: b, kind });
        }
    };
    let mut stack: Vec<&Value> = p.doc.values().collect();
    while let Some(v) = stack.pop() {
        match v {
            Value::Scalar(_) => {}
            Value::Array(a) => {
                if a.kind != ArrayKind::Block {
                    fold(a.span.start, a.span.end, FoldKind::Region);
                }
                for item in &a.items {
                    if let Some(value) = &item.value {
                        if !matches!(value, Value::Scalar(_)) {
                            fold(item.span.start, item.span.end, FoldKind::Region);
                        }
                        stack.push(value);
                    }
                }
            }
            Value::Entity(e) => {
                stack.push(&e.head);
                if let Some(a) = &e.attributes {
                    fold(a.span.start, a.span.end, FoldKind::Region);
                    stack.extend(a.items.iter().filter_map(|i| i.value.as_ref()));
                }
            }
            Value::Chain(links) => {
                for l in links {
                    if let Some(a) = &l.attributes {
                        fold(a.span.start, a.span.end, FoldKind::Region);
                        stack.extend(a.items.iter().filter_map(|i| i.value.as_ref()));
                    }
                }
            }
        }
    }
    for t in &p.lexed.tokens {
        if matches!(t.kind, TokenKind::String | TokenKind::Error) {
            fold(t.span.start, t.span.end, FoldKind::Region);
        }
    }
    comment_folds(p, &mut out);
    out.sort_by_key(|f| (f.start_line, std::cmp::Reverse(f.end_line)));
    out.dedup_by_key(|f| (f.start_line, f.end_line));
    out
}

/// Runs of two or more lines holding only a comment.
fn comment_folds(p: &Parsed, out: &mut Vec<Fold>) {
    let mut run: Option<(u32, u32)> = None;
    let own_line = |start: u32| {
        let line = p.line(start);
        let from = p.lines.line_start(line) as usize;
        p.text[from..start as usize].bytes().all(|c| c == b' ' || c == b'\t').then_some(line)
    };
    for c in &p.lexed.comments {
        let line = own_line(c.start);
        run = match (run, line) {
            (Some((a, b)), Some(l)) if l == b + 1 => Some((a, l)),
            (prev, l) => {
                if let Some((a, b)) = prev.filter(|(a, b)| b > a) {
                    out.push(Fold { start_line: a, end_line: b, kind: FoldKind::Comment });
                }
                l.map(|l| (l, l))
            }
        };
    }
    if let Some((a, b)) = run.filter(|(a, b)| b > a) {
        out.push(Fold { start_line: a, end_line: b, kind: FoldKind::Comment });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(text: &str) -> Vec<String> {
        let p = Parsed::new(text.to_owned());
        let symbols = outline(&p);
        symbols
            .iter()
            .map(|s| {
                let mut depth = 0;
                let mut at = s.parent;
                while let Some(i) = at {
                    depth += 1;
                    at = symbols[i].parent;
                }
                let detail = s.detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default();
                format!("{}{} {}{detail}", "  ".repeat(depth), s.kind.name(), s.name)
            })
            .collect()
    }

    #[test]
    fn nested_keys_and_services() {
        let text = "parameters:\n\tmailer:\n\t\thost: smtp\nservices:\n\tdb: PDO('sqlite::memory:')\n\t- App\\Router::create\n\tarticles:\n\t\tcreate: App\\Articles(%a%)\n\t\tsetup:\n\t\t\t- init()\n";
        assert_eq!(
            tree(text),
            [
                "module parameters",
                "  key mailer",
                "    property host (smtp)",
                "module services",
                "  class db (PDO)",
                "  class App\\Router::create",
                "  class articles (App\\Articles)",
                "    property create (App\\Articles(…))",
                "    key setup",
                "      enum-member init",
            ]
        );
    }

    #[test]
    fn list_entries_are_named_by_their_first_value() {
        let text = "parameters:\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: '#^Foo#'\n\t\t\tpath: a.php\n\t\t- '#^Bar#'\n";
        let t = tree(text);
        assert_eq!(t[2], "    struct #^Foo# (message)");
        assert_eq!(t[3], "      property message (#^Foo#)");
        assert_eq!(t[5], "    enum-member #^Bar#");
    }

    #[test]
    fn blank_strings_are_named_as_written() {
        assert_eq!(
            tree("'': 1\nx: ' '\ny:\n\t- \"\"\n"),
            ["module '' (1)", "module x (' ')", "module y", "  enum-member \"\""]
        );
    }

    #[test]
    fn huge_lists_are_cut() {
        let text: String =
            std::iter::once("list:\n".to_owned()).chain((0..1200).map(|i| format!("\t- {i}\n"))).collect();
        let p = Parsed::new(text);
        let symbols = outline(&p);
        assert_eq!(symbols.len(), 1 + MAX_CHILDREN + 1);
        assert_eq!(symbols.last().map(|s| s.name.as_str()), Some("… 700 more"));
    }

    #[test]
    fn a_broken_line_keeps_the_outline() {
        let text = "a:\n\tb: [1, 2\nc: 3\n";
        assert_eq!(tree(text), ["module a", "  property b ([1, …])", "module c (3)"]);
        // A multiline string not closed yet, as while typing it.
        let text = "parameters:\n\tmessage: \"\"\"\n\t\tHello\nservices:\n\t- App\\Foo\n";
        assert_eq!(
            tree(text),
            ["module parameters", "  property message (\"\"\"…)", "module services", "  class App\\Foo"]
        );
    }

    #[test]
    fn folds() {
        let text = "# one\n# two\na:\n\tb: 1\n\tc: [\n\t\tx\n\t]\nd: '''\n\ttext\n\t'''\ne: Foo(\n\t1\n)\n";
        let p = Parsed::new(text.to_owned());
        let folds: Vec<(u32, u32, FoldKind)> = folding(&p).iter().map(|f| (f.start_line, f.end_line, f.kind)).collect();
        assert_eq!(
            folds,
            [
                (0, 1, FoldKind::Comment),
                (2, 6, FoldKind::Region),
                (4, 6, FoldKind::Region),
                (7, 9, FoldKind::Region),
                (10, 12, FoldKind::Region),
            ]
        );
    }
}
