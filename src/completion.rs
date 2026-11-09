//! Completion: keys of the block the caret is in (Nette or PHPStan, by the
//! file), `%parameters%` and `@services` of the file and its includes, values
//! of known keys, and files in `includes:`.

use crate::config::{Step, line_context};
use crate::schema::{self, Flavor, Hint};
use crate::workspace::{Files, Loaded, dir_of, relative};
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::BTreeSet;
use std::rc::Rc;

const MAX_ITEMS: usize = 500;

pub fn complete(files: &mut Files, doc: Rc<Loaded>, offset: usize) -> Vec<CompletionItem> {
    let p = &doc.parsed;
    let text = &p.text;
    let line_start = p.lines.line_start(p.lines.line_of(offset)) as usize;
    let before = &text[line_start..offset];
    let flavor = schema::flavor(&doc.path, &doc.model);

    if let Some(start) = parameter_start(before) {
        let (from, to, closed) = word_after(text, line_start + start, offset, |c| is_name(c) || c == '.' || c == '-');
        let related = files.related(doc.clone());
        let mut names = BTreeSet::new();
        let mut items = Vec::new();
        for file in &related {
            for d in &file.model.parameters {
                if names.insert(d.name.clone()) {
                    let mut item = CompletionItem::new(&d.name, CompletionKind::Variable);
                    item.detail = Some(file_label(&doc.path, &file.path));
                    item.documentation = d.value.map(|v| format!("```neon\n{}\n```", excerpt(&file.parsed.text, v)));
                    items.push(item);
                }
            }
        }
        let builtins: &[(&str, &str)] = match flavor {
            Flavor::PhpStan => schema::PHPSTAN_BUILTINS,
            _ => schema::NETTE_PARAMETERS,
        };
        for (name, doc) in builtins {
            if names.insert((*name).to_owned()) {
                let mut item = CompletionItem::new(name, CompletionKind::Constant);
                item.detail = Some("built-in".into());
                item.documentation = Some((*doc).to_owned());
                items.push(item);
            }
        }
        return finish(p, items, from, to, if closed { "" } else { "%" });
    }

    if let Some(start) = service_start(before) {
        let (from, to, _) = word_after(text, line_start + start, offset, |c| is_name(c) || c == '.' || c == '\\');
        let mut names = BTreeSet::new();
        let mut items = Vec::new();
        for file in files.related(doc.clone()) {
            for d in &file.model.services {
                if names.insert(d.name.clone()) {
                    let mut item = CompletionItem::new(&d.name, CompletionKind::Reference);
                    item.detail = d.class.clone();
                    item.documentation = Some(format!("Service in `{}`", file_label(&doc.path, &file.path)));
                    items.push(item);
                }
            }
        }
        return finish(p, items, from, to, "");
    }

    let ctx = line_context(p, offset);
    if ctx.inline {
        return literals(p, offset, None);
    }
    if ctx.at_key {
        let (from, to, _) = word_around(text, offset, |c| is_name(c) || matches!(c, '.' | '-' | '!' | '?' | '$'));
        let includes = matches!(ctx.path.as_slice(), [Step::Key(k), Step::Item] if k == "includes");
        if includes {
            return include_files(&doc, from, to);
        }
        let keys = schema::keys_at(&ctx.path, flavor);
        let top = ctx.path.is_empty();
        let items = keys
            .into_iter()
            .filter(|k| !ctx.siblings.iter().any(|s| s == k.name))
            .map(|k| {
                let mut item =
                    CompletionItem::new(k.name, if top { CompletionKind::Module } else { CompletionKind::Property });
                item.insert_text = if k.hint == Hint::Block { format!("{}:", k.name) } else { format!("{}: ", k.name) };
                item.documentation = Some(k.doc.to_owned());
                item.detail = Some(origin(&ctx.path, flavor).into());
                item
            })
            .collect();
        return finish(p, items, from, to, "");
    }
    if let Some((key, _)) = &ctx.key {
        let hint = schema::key_doc(&ctx.path, key, flavor).map(|k| k.hint);
        return literals(p, offset, Some((key.as_str(), hint)));
    }
    Vec::new()
}

fn is_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()
}

/// Where the parameter name being typed starts: after an opening `%`.
fn parameter_start(before: &str) -> Option<usize> {
    let name_start = before.trim_end_matches(|c: char| is_name(c) || c == '.' || c == '-').len();
    let percent = name_start.checked_sub(1).filter(|&i| before.as_bytes()[i] == b'%')?;
    // An even number of `%` before it: this one opens a reference (`%%` are pairs).
    before[..percent].matches('%').count().is_multiple_of(2).then_some(name_start)
}

/// Where the service name being typed starts: after an `@` at a word's start.
fn service_start(before: &str) -> Option<usize> {
    let name_start = before.trim_end_matches(|c: char| is_name(c) || c == '.' || c == '\\').len();
    let at = name_start.checked_sub(1).filter(|&i| before.as_bytes()[i] == b'@')?;
    let prev = before[..at].chars().next_back();
    prev.is_none_or(|c| !is_name(c)).then_some(name_start)
}

/// The name from `from` to the end of what is typed after `offset`, and
/// whether a `%` already closes it.
fn word_after(text: &str, from: usize, offset: usize, name: impl Fn(char) -> bool) -> (usize, usize, bool) {
    let rest = &text[offset..];
    let len = rest.find(|c: char| !name(c)).unwrap_or(rest.len());
    let to = offset + len;
    (from, to, text[to..].starts_with('%'))
}

fn word_around(text: &str, offset: usize, name: impl Fn(char) -> bool) -> (usize, usize, bool) {
    let before = &text[..offset];
    let from = before.trim_end_matches(&name).len();
    word_after(text, from, offset, name)
}

fn finish(
    p: &crate::neon::Parsed,
    mut items: Vec<CompletionItem>,
    from: usize,
    to: usize,
    suffix: &str,
) -> Vec<CompletionItem> {
    let range = p.lines.range(&p.text, from, to);
    items.truncate(MAX_ITEMS);
    for item in &mut items {
        item.insert_text.push_str(suffix);
        item.replace = Some(range);
    }
    items
}

/// `true`, `false`, `null`, or the values a known key takes.
fn literals(p: &crate::neon::Parsed, offset: usize, key: Option<(&str, Option<Hint>)>) -> Vec<CompletionItem> {
    let (from, to, _) = word_around(&p.text, offset, |c| c.is_ascii_alphanumeric());
    let words: Vec<&str> = match key {
        Some((_, Some(Hint::Level))) => vec!["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "max"],
        Some((_, Some(Hint::Bool))) => vec!["true", "false"],
        Some(("autowired", _)) => vec!["true", "false", "self"],
        _ => vec!["true", "false", "null"],
    };
    let items = words
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            let kind =
                if w.chars().all(|c| c.is_ascii_digit()) { CompletionKind::Value } else { CompletionKind::Keyword };
            let mut item = CompletionItem::new(w, kind);
            item.sort_text = Some(format!("{i:03}"));
            item
        })
        .collect();
    finish(p, items, from, to, "")
}

/// NEON files of the project, relative to the document.
fn include_files(doc: &Loaded, from: usize, to: usize) -> Vec<CompletionItem> {
    let Some(project) = openidea_plugin::host::project_of(&doc.path) else { return Vec::new() };
    let dir = dir_of(&doc.path);
    let found = project.find_files("**/*.neon*", 300).unwrap_or_default();
    let items = found
        .iter()
        .map(|path| path.replace('\\', "/"))
        .filter(|path| (path.ends_with(".neon") || path.ends_with(".neon.dist")) && *path != doc.path)
        .map(|path| {
            let rel = relative(dir, &path);
            let mut item = CompletionItem::new(&rel, CompletionKind::File);
            item.sort_text = Some(format!("{:04}{rel}", rel.matches("..").count()));
            item
        })
        .collect();
    finish(&doc.parsed, items, from, to, "")
}

fn origin(path: &[Step], flavor: Flavor) -> &'static str {
    match (path.first(), flavor) {
        (Some(Step::Key(k)), _) if k == "services" => "Nette DI service",
        (_, Flavor::PhpStan) => "PHPStan",
        (None, Flavor::Both) => "Nette / PHPStan",
        (Some(Step::Key(k)), Flavor::Both) if k == "parameters" => "PHPStan",
        _ => "Nette",
    }
}

/// How another file is named next to the document.
pub fn file_label(from: &str, path: &str) -> String {
    if from == path { "this file".into() } else { relative(dir_of(from), path) }
}

/// The text of a span, at most 8 lines and 400 bytes.
pub fn excerpt(text: &str, span: crate::neon::Span) -> String {
    let s = span.text(text);
    let mut out: String = s.lines().take(8).collect::<Vec<_>>().join("\n");
    if out.len() > 400 {
        let mut cut = 400;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push('…');
    } else if s.lines().count() > 8 {
        out.push_str("\n…");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn complete_at(path: &str, text: &str) -> Vec<CompletionItem> {
        let offset = text.find('|').unwrap();
        let text = text.replacen('|', "", 1);
        let doc = Rc::new(Loaded::new(path.into(), text));
        complete(&mut Files::default(), doc, offset)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn top_level_sections_by_file() {
        let _host = MockHost::install();
        let nette = complete_at("/project/config/common.neon", "application:\n\tmapping: x\n|");
        assert!(labels(&nette).contains(&"database"));
        assert!(!labels(&nette).contains(&"application"), "already there");
        assert!(!labels(&nette).contains(&"parametersSchema"));
        let phpstan = complete_at("/project/phpstan.neon", "par|");
        assert!(labels(&phpstan).contains(&"parametersSchema"));
        assert!(!labels(&phpstan).contains(&"latte"));
        let item = phpstan.iter().find(|i| i.label == "parameters").unwrap();
        assert_eq!(item.insert_text, "parameters:");
        assert_eq!(item.replace.map(|r| (r.start.character, r.end.character)), Some((0, 3)));
    }

    #[test]
    fn keys_of_services_and_phpstan_parameters() {
        let _host = MockHost::install();
        let s = complete_at("/project/services.neon", "services:\n\tdb:\n\t\tcreate: PDO\n\t\t|");
        assert!(labels(&s).contains(&"setup") && !labels(&s).contains(&"create"));
        let s = complete_at("/project/services.neon", "services:\n\t- create: Foo\n\t  aut|");
        assert!(labels(&s).contains(&"autowired"));
        let p = complete_at("/project/phpstan.neon", "parameters:\n\tlevel: 5\n\t|");
        assert!(labels(&p).contains(&"treatPhpDocTypesAsCertain") && !labels(&p).contains(&"level"));
        assert_eq!(p.iter().find(|i| i.label == "tmpDir").map(|i| i.insert_text.as_str()), Some("tmpDir: "));
        let e = complete_at("/project/phpstan.neon", "parameters:\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: x\n\t\t\t|");
        assert!(
            labels(&e).contains(&"identifier") && labels(&e).contains(&"count") && !labels(&e).contains(&"message")
        );
        let e = complete_at("/project/phpstan.neon", "parameters:\n\tignoreErrors:\n\t\t- m|");
        assert!(labels(&e).contains(&"reportUnmatched"));
    }

    #[test]
    fn values_of_known_keys() {
        let _host = MockHost::install();
        let v = complete_at("/project/phpstan.neon", "parameters:\n\tlevel: |");
        assert_eq!(labels(&v)[..3], ["0", "1", "2"]);
        assert!(labels(&v).contains(&"max"));
        let v = complete_at("/project/phpstan.neon", "parameters:\n\treportUnmatchedIgnoredErrors: t|");
        assert_eq!(labels(&v), ["true", "false"]);
        let v = complete_at("/project/a.neon", "x: |");
        assert_eq!(labels(&v), ["true", "false", "null"]);
        let v = complete_at("/project/a.neon", "services:\n\tfoo:\n\t\tautowired: |");
        assert_eq!(labels(&v), ["true", "false", "self"]);
    }

    #[test]
    fn parameters_from_the_file_and_its_includes() {
        let host = MockHost::install();
        host.add_file("/project/config/local.neon", "parameters:\n\tdatabase:\n\t\tuser: root\n");
        let text = "includes:\n\t- local.neon\nparameters:\n\tmailer:\n\t\thost: x\nservices:\n\t- Foo(%ma|)\n";
        let items = complete_at("/project/config/common.neon", text);
        let names = labels(&items);
        for name in ["mailer", "mailer.host", "database", "database.user", "appDir", "tempDir"] {
            assert!(names.contains(&name), "{name}: {names:?}");
        }
        let host_item = items.iter().find(|i| i.label == "mailer.host").unwrap();
        assert_eq!(host_item.insert_text, "mailer.host%");
        assert_eq!(
            items.iter().find(|i| i.label == "database.user").and_then(|i| i.detail.as_deref()),
            Some("local.neon")
        );
        // Already closed: no second `%`.
        let items = complete_at("/project/a.neon", "parameters:\n\tfoo: 1\nx: '%f|%/x'");
        assert_eq!(items.iter().find(|i| i.label == "foo").map(|i| i.insert_text.as_str()), Some("foo"));
        // `%%` is an escaped percent, not a reference.
        assert!(complete_at("/project/a.neon", "x: '100%%|'").iter().all(|i| i.label != "appDir"));
    }

    #[test]
    fn services_after_an_at_sign() {
        let _host = MockHost::install();
        let text = "services:\n\tdatabase: PDO('sqlite::memory:')\n\trouter:\n\t\tcreate: App\\Router\n\t- Foo(@|)\n";
        let items = complete_at("/project/services.neon", text);
        assert_eq!(labels(&items), ["database", "router"]);
        assert_eq!(items[1].detail.as_deref(), Some("App\\Router"));
    }

    #[test]
    fn include_paths() {
        let host = MockHost::install();
        host.add_file("/project/config/services.neon", "");
        host.add_file("/project/phpstan-baseline.neon", "");
        let items = complete_at("/project/config/common.neon", "includes:\n\t- |\n");
        let names = labels(&items);
        assert!(names.contains(&"services.neon") && names.contains(&"../phpstan-baseline.neon"), "{names:?}");
    }

    #[test]
    fn positions_count_utf16_units() {
        let _host = MockHost::install();
        let text = "parameters:\n\tžluť😀: 1\nx: '😀 %žl|'";
        let items = complete_at("/project/a.neon", text);
        let item = items.iter().find(|i| i.label == "žluť😀").unwrap();
        // `x: '😀 %` is 3 + 1 + 2 + 2 = 8 units.
        assert_eq!(item.replace.map(|r| (r.start.line, r.start.character, r.end.character)), Some((2, 8, 10)));
    }
}
