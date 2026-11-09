//! What a configuration file defines: included files, parameters (`%a.b%`)
//! and named services (`@name`), read from its tree; and the chain of keys
//! leading to a line, read from the tokens so it works while the line is
//! still being typed.

use crate::neon::{Array, ArrayKind, Parsed, Span, TokenKind, Value};

/// Parameters of one file at most (a baseline has one; this is a guard).
const MAX_PARAMETERS: usize = 10_000;
const MAX_PARAMETER_DEPTH: usize = 16;

#[derive(Debug, Default)]
pub struct Model {
    /// `includes:` entries: the path as written and its token.
    pub includes: Vec<(String, Span)>,
    pub parameters: Vec<Definition>,
    pub services: Vec<Definition>,
    /// Top-level keys.
    pub sections: Vec<String>,
}

/// A named thing and where it is defined.
#[derive(Debug, Clone)]
pub struct Definition {
    /// `a.b` for a nested parameter.
    pub name: String,
    /// The key token.
    pub key: Span,
    /// The whole item, key to value.
    pub item: Span,
    /// The value, as written.
    pub value: Option<Span>,
    /// For a service: its class or factory.
    pub class: Option<String>,
}

impl Model {
    pub fn new(p: &Parsed) -> Self {
        let mut m = Model::default();
        for root in p.doc.values() {
            let Value::Array(a) = root else { continue };
            for item in &a.items {
                let Some(k) = item.key else { continue };
                let name = p.key_name(k).into_owned();
                match (name.as_str(), item.value.as_ref()) {
                    ("includes", Some(Value::Array(list))) => {
                        for entry in &list.items {
                            if let Some(t) = entry.value.as_ref().and_then(Value::scalar) {
                                m.includes.push((p.scalar_text(t).into_owned(), p.lexed.tokens[t as usize].span));
                            }
                        }
                    }
                    ("parameters", Some(Value::Array(params))) => parameters(p, params, "", 0, &mut m.parameters),
                    ("services", Some(Value::Array(services))) => {
                        for s in services.items.iter() {
                            let Some(k) = s.key else { continue };
                            m.services.push(Definition {
                                name: p.key_name(k).into_owned(),
                                key: p.lexed.tokens[k as usize].span,
                                item: s.span,
                                value: s.value.as_ref().map(|v| v.span(&p.lexed.tokens)),
                                class: crate::outline::service_class(p, s),
                            });
                        }
                    }
                    _ => {}
                }
                m.sections.push(name);
            }
        }
        m
    }

    pub fn parameter(&self, name: &str) -> Option<&Definition> {
        self.parameters.iter().find(|d| d.name == name)
    }

    pub fn service(&self, name: &str) -> Option<&Definition> {
        self.services.iter().find(|d| d.name == name)
    }
}

fn parameters(p: &Parsed, array: &Array, prefix: &str, depth: usize, out: &mut Vec<Definition>) {
    if depth > MAX_PARAMETER_DEPTH {
        return;
    }
    for item in &array.items {
        if out.len() >= MAX_PARAMETERS {
            return;
        }
        let Some(k) = item.key else { continue };
        let name = format!("{prefix}{}", p.key_name(k));
        out.push(Definition {
            name: name.clone(),
            key: p.lexed.tokens[k as usize].span,
            item: item.span,
            value: item.value.as_ref().map(|v| v.span(&p.lexed.tokens)),
            class: None,
        });
        if let Some(Value::Array(nested)) = &item.value {
            parameters(p, nested, &format!("{name}."), depth + 1, out);
        }
    }
}

/// One step of the way from the top of a document to a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(String),
    /// An item of a list (`-`).
    Item,
    /// Inside the arguments of an entity: keys there are argument names.
    Arguments,
}

/// What the line at `offset` is, judged by its tokens.
#[derive(Debug, PartialEq, Eq)]
pub struct LineContext {
    /// The keys (and list items) the line is nested in.
    pub path: Vec<Step>,
    /// The key the line starts with (after dashes), when its colon is before `offset`.
    pub key: Option<(String, Span)>,
    /// `offset` is where a key goes: the start of the line or after `- `, or in the word typed there.
    pub at_key: bool,
    /// The offset is inside brackets, where indentation means nothing.
    pub inline: bool,
    /// Keys of the same block, for not offering them again.
    pub siblings: Vec<String>,
}

/// The shape of a line, from its first token.
struct Line<'p> {
    indent: &'p str,
    dash: bool,
    /// The key on the line (after a dash).
    key: Option<String>,
}

fn shape<'p>(p: &'p Parsed, first: usize) -> Line<'p> {
    let toks = &p.lexed.tokens;
    let t = toks[first];
    let indent = &p.text[t.line as usize..t.span.start as usize];
    let dash = t.kind == TokenKind::Dash;
    let k = if dash { first + 1 } else { first };
    let key = (|| {
        let key = toks.get(k)?;
        let colon = toks.get(k + 1)?;
        let same_line = !key.newline || k == first;
        (same_line
            && matches!(key.kind, TokenKind::Literal | TokenKind::String)
            && matches!(colon.kind, TokenKind::Colon | TokenKind::Equals)
            && !colon.newline)
            .then(|| p.key_name(k as u32).into_owned())
    })();
    Line { indent, dash, key }
}

/// The indentation of the lines continuing a dash line indented `dash`, as
/// nette/neon decides it: a tab when the next line has one there, else two spaces.
fn dash_continuation(dash: &str, next: &str) -> usize {
    dash.len() + if next.len() > dash.len() && next.as_bytes()[dash.len()] == b'\t' { 1 } else { 2 }
}

pub fn line_context(p: &Parsed, offset: usize) -> LineContext {
    let toks = &p.lexed.tokens;
    let mut depth = 0usize;
    let mut starts: Vec<usize> = Vec::new(); // first tokens of lines outside brackets, before the offset
    let mut end = 0;
    for (i, t) in toks.iter().enumerate() {
        if t.span.start as usize >= offset {
            break;
        }
        if (i == 0 || t.newline) && depth == 0 {
            starts.push(i);
        }
        if t.kind.is_open() {
            depth += 1;
        } else if t.kind.is_close() {
            depth = depth.saturating_sub(1);
        }
        end = i + 1;
    }
    let line = p.lines.line_of(offset);
    let line_start = p.lines.line_start(line) as usize;
    let current = starts.last().copied().filter(|&i| p.line(toks[i].span.start) == line);
    let before_offset = |i: usize| toks.get(i).is_some_and(|t| (t.span.end as usize) <= offset && i < end);
    let mut ctx = LineContext { path: Vec::new(), key: None, at_key: false, inline: depth > 0, siblings: Vec::new() };
    let indent: &str = match current {
        Some(i) => shape(p, i).indent,
        None => {
            let ws = p.text[line_start..offset].bytes().take_while(|&c| c == b' ' || c == b'\t').count();
            &p.text[line_start..line_start + ws]
        }
    };
    let mut inner = Vec::new();
    match current {
        Some(first) => {
            let mut i = first;
            while toks.get(i).is_some_and(|t| t.kind == TokenKind::Dash) && before_offset(i) {
                inner.push(Step::Item);
                i += 1;
            }
            let word = toks.get(i).filter(|t| i < end && !(t.newline && i > first));
            let colon =
                toks.get(i + 1).filter(|t| matches!(t.kind, TokenKind::Colon | TokenKind::Equals) && !t.newline);
            match (word, colon) {
                (Some(w), Some(_))
                    if before_offset(i + 1) && matches!(w.kind, TokenKind::Literal | TokenKind::String) =>
                {
                    ctx.key = Some((p.key_name(i as u32).into_owned(), w.span));
                }
                (Some(w), _) => {
                    ctx.at_key = matches!(w.kind, TokenKind::Literal | TokenKind::String) && i + 1 >= end;
                }
                (None, _) => ctx.at_key = true,
            }
        }
        None => ctx.at_key = p.text[line_start..offset].trim().is_empty(),
    }
    ctx.at_key &= !ctx.inline;
    let dashed = !inner.is_empty();
    // Lines above: the same block gives siblings, less indented ones the way up.
    let mut target = indent;
    let mut same_block = !dashed;
    let above = starts.iter().rev().filter(|&&i| Some(i) != current);
    let mut path = Vec::new();
    for &i in above {
        let l = shape(p, i);
        if l.indent.len() >= target.len() {
            if same_block
                && l.indent.len() == target.len()
                && let Some(key) = l.key.filter(|_| !l.dash)
            {
                ctx.siblings.push(key);
            }
            continue;
        }
        if l.dash {
            let continuation = dash_continuation(l.indent, target);
            match l.key {
                Some(key) if target.len() > continuation => {
                    path.push(Step::Key(key));
                }
                Some(key) if same_block => ctx.siblings.push(key),
                _ => {}
            }
            path.push(Step::Item);
        } else if let Some(key) = l.key {
            path.push(Step::Key(key));
        } else {
            break;
        }
        same_block = false;
        target = l.indent;
        if target.is_empty() {
            break;
        }
    }
    path.reverse();
    path.extend(inner);
    ctx.path = path;
    // Lines below in the same block.
    if !dashed {
        let mut d = depth;
        for (i, t) in toks.iter().enumerate().skip(end) {
            if t.newline && d == 0 && p.line(t.span.start) > line {
                let l = shape(p, i);
                if l.indent.len() < indent.len() || ctx.siblings.len() > 200 {
                    break;
                }
                if l.indent.len() == indent.len()
                    && let Some(key) = l.key.filter(|_| !l.dash)
                {
                    ctx.siblings.push(key);
                }
            }
            if t.kind.is_open() {
                d += 1;
            } else if t.kind.is_close() {
                d = d.saturating_sub(1);
            }
        }
    }
    ctx
}

/// The block arrays of the tree holding `offset`, outermost first, with the key of each step.
pub fn path_at(p: &Parsed, offset: usize) -> Vec<Step> {
    let mut path = Vec::new();
    let Some(Value::Array(root)) = p.doc.root.as_ref() else { return path };
    let mut array = root;
    loop {
        let Some(item) = array.items.iter().find(|i| i.span.contains(offset)) else { return path };
        match item.key {
            Some(k) if p.lexed.tokens[k as usize].span.contains(offset) => return path,
            Some(k) => path.push(Step::Key(p.key_name(k).into_owned())),
            None => path.push(Step::Item),
        }
        match &item.value {
            Some(Value::Array(a)) if a.kind != ArrayKind::Paren => array = a,
            Some(v @ (Value::Entity(_) | Value::Chain(_) | Value::Array(_)))
                if v.span(&p.lexed.tokens).contains(offset) =>
            {
                path.push(Step::Arguments);
                return path;
            }
            _ => return path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(path: &[Step]) -> Vec<String> {
        path.iter()
            .map(|s| match s {
                Step::Key(k) => k.clone(),
                Step::Item => "-".into(),
                Step::Arguments => "()".into(),
            })
            .collect()
    }

    /// The context at `|` in the text.
    fn at(text: &str) -> LineContext {
        let offset = text.find('|').unwrap();
        let text = text.replacen('|', "", 1);
        let p = Parsed::new(text);
        line_context(&p, offset)
    }

    #[test]
    fn the_model_of_a_file() {
        let p = Parsed::new(
            "includes:\n\t- a.neon\n\t- 'b.neon'\nparameters:\n\tmailer!:\n\t\thost: x\n\tlist: [1, 2]\nservices:\n\tdb: PDO()\n\t- Foo\n\trouter:\n\t\tcreate: App\\Router\n".into(),
        );
        let m = Model::new(&p);
        assert_eq!(m.includes.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["a.neon", "b.neon"]);
        let names: Vec<&str> = m.parameters.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["mailer", "mailer.host", "list"]);
        let services: Vec<(&str, Option<&str>)> =
            m.services.iter().map(|d| (d.name.as_str(), d.class.as_deref())).collect();
        assert_eq!(services, [("db", Some("PDO")), ("router", Some("App\\Router"))]);
        assert_eq!(m.sections, ["includes", "parameters", "services"]);
    }

    #[test]
    fn contexts_of_lines() {
        let c = at("parameters:\n\tlevel: 5\n\tlev|\n");
        assert_eq!(
            (keys(&c.path), c.at_key, c.siblings.clone()),
            (vec!["parameters".into()], true, vec!["level".into()])
        );
        let c = at("parameters:\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: x\n\t\t\t|\n");
        assert_eq!(keys(&c.path), ["parameters", "ignoreErrors", "-"]);
        assert_eq!(c.siblings, ["message"]);
        let c = at("services:\n\t- factory: Foo\n\t  se|\n");
        assert_eq!(keys(&c.path), ["services", "-"]);
        assert_eq!(c.siblings, ["factory"]);
        let c = at("services:\n\t- |\n");
        assert_eq!((keys(&c.path), c.at_key), (vec!["services".to_owned(), "-".to_owned()], true));
        let c = at("services:\n\tfoo:\n\t\tautowired: |\n");
        assert_eq!(keys(&c.path), ["services", "foo"]);
        assert_eq!(c.key.as_ref().map(|k| k.0.as_str()), Some("autowired"));
        assert!(!c.at_key);
        let c = at("a: [\n\tb, |\n]\n");
        assert!(c.inline);
        let c = at("|");
        assert!(c.at_key && c.path.is_empty());
        let c = at("- name: John\n  ag|\n");
        assert_eq!(keys(&c.path), ["-"]);
        let c = at("search:\n\t-\tin: x\n\t\tcl|\n");
        assert_eq!(keys(&c.path), ["search", "-"]);
        let c = at("a:\n\t# comment\n\n\tb:\n\t\t|");
        assert_eq!(keys(&c.path), ["a", "b"]);
    }

    #[test]
    fn paths_in_the_tree() {
        let text = "parameters:\n\tparallel:\n\t\tjobSize: 20\n";
        let p = Parsed::new(text.into());
        assert_eq!(keys(&path_at(&p, text.find("jobSize").unwrap())), ["parameters", "parallel"]);
        assert_eq!(keys(&path_at(&p, text.find("20").unwrap())), ["parameters", "parallel", "jobSize"]);
        let text = "services:\n\t- Foo(create: 1, x: [a: 1])\n";
        let p = Parsed::new(text.into());
        assert_eq!(keys(&path_at(&p, text.find("create").unwrap())), ["services", "-", "()"]);
    }
}
