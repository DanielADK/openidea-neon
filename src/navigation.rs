//! Go to Definition and hover: included files, `%parameters%` and `@services`
//! across includes, PHP classes from the project index, known keys.

use crate::completion::{excerpt, file_label};
use crate::config::{Definition, Step};
use crate::neon::Span;
use crate::references::{self, Reference};
use crate::schema::{self, Flavor};
use crate::workspace::{Files, Loaded, include_path};
use openidea_plugin::hover::Hover;
use openidea_plugin::{Location, Position, Range};
use std::rc::Rc;

pub fn definition(files: &mut Files, doc: Rc<Loaded>, offset: usize) -> Vec<Location> {
    let Some((reference, _)) = references::at(&doc.parsed, offset) else { return Vec::new() };
    match reference {
        Reference::Include(entry) => {
            let Some(path) = include_path(&doc.path, &entry) else { return Vec::new() };
            let exists = openidea_plugin::host::project_of(&path).and_then(|p| p.exists(&path).ok()).unwrap_or(false);
            if exists { vec![Location { path, range: Range::default() }] } else { Vec::new() }
        }
        Reference::Parameter(name) => {
            definitions(files, doc, |file| file.model.parameter(&name).cloned()).iter().map(location).collect()
        }
        Reference::Service(name) => {
            let found: Vec<Location> = definitions(files, doc.clone(), |file| file.model.service(&name).cloned())
                .iter()
                .map(location)
                .collect();
            if found.is_empty() && crate::highlight::is_class_name(&name) { classes(&doc, &name) } else { found }
        }
        Reference::Class(fqn) => classes(&doc, &fqn),
        Reference::Key { .. } => Vec::new(),
    }
}

/// Definitions in the document and its includes, the one that wins first.
fn definitions(
    files: &mut Files,
    doc: Rc<Loaded>,
    find: impl Fn(&Loaded) -> Option<Definition>,
) -> Vec<(Rc<Loaded>, Definition)> {
    // The document itself needs no other file.
    if let Some(d) = find(&doc)
        && doc.model.includes.is_empty()
    {
        return vec![(doc, d)];
    }
    files.related(doc).into_iter().filter_map(|file| find(&file).map(|d| (file, d))).collect()
}

fn location((file, d): &(Rc<Loaded>, Definition)) -> Location {
    let p = &file.parsed;
    Location { path: file.path.clone(), range: p.lines.range(&p.text, d.key.start as usize, d.key.end as usize) }
}

fn classes(doc: &Loaded, fqn: &str) -> Vec<Location> {
    let Some(project) = openidea_plugin::host::project_of(&doc.path) else { return Vec::new() };
    references::find_class(&project, fqn).into_iter().map(|s| s.location).collect()
}

pub fn hover(files: &mut Files, doc: Rc<Loaded>, offset: usize) -> Option<Hover> {
    let (reference, span) = references::at(&doc.parsed, offset)?;
    let range = doc.parsed.lines.range(&doc.parsed.text, span.start as usize, span.end as usize);
    let flavor = schema::flavor(&doc.path, &doc.model);
    let markdown = match reference {
        Reference::Key { path, name } => key_hover(&path, &name, flavor)?,
        Reference::Parameter(name) => parameter_hover(files, doc.clone(), &name, flavor),
        // `@App\Foo` with no service of that name is the service of that type.
        Reference::Service(name) => match service_hover(files, doc.clone(), &name) {
            Some(markdown) => markdown,
            None if crate::highlight::is_class_name(&name) => class_hover(&doc, &name)?,
            None => return None,
        },
        Reference::Class(fqn) => class_hover(&doc, &fqn)?,
        Reference::Include(entry) => {
            let path = include_path(&doc.path, &entry)?;
            format!("Includes `{}`", file_label(&doc.path, &path))
        }
    };
    Some(Hover::new(markdown).range(range))
}

fn key_hover(path: &[Step], name: &str, flavor: Flavor) -> Option<String> {
    let key = schema::key_doc(path, name, flavor)?;
    Some(format!("**{}** — {}", key.name, key.doc))
}

fn where_defined(doc: &Loaded, file: &Loaded, key: Span) -> String {
    let line = file.parsed.line(key.start) + 1;
    format!("`{}`, line {line}", file_label(&doc.path, &file.path))
}

fn more(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => " (overrides 1 more definition)".into(),
        n => format!(" (overrides {n} more definitions)"),
    }
}

fn parameter_hover(files: &mut Files, doc: Rc<Loaded>, name: &str, flavor: Flavor) -> String {
    let found = definitions(files, doc.clone(), |file| file.model.parameter(name).cloned());
    if let Some((file, d)) = found.first() {
        let value = d.value.map(|v| excerpt(&file.parsed.text, v)).unwrap_or_else(|| "null".into());
        let value = if value.contains('\n') {
            format!("\n\n```neon\n{value}\n```")
        } else {
            format!(" = `{}`", value.replace('`', "'"))
        };
        return format!(
            "**%{name}%**{value}\n\nDefined in {}{}",
            where_defined(&doc, file, d.key),
            more(found.len() - 1)
        );
    }
    let builtin = |list: &[(&'static str, &'static str)]| {
        let root = name.split('.').next().unwrap_or(name);
        list.iter().find(|(n, _)| *n == name || (*n == "env" && root == "env")).map(|(_, d)| *d)
    };
    let doc_line = match flavor {
        Flavor::PhpStan => builtin(schema::PHPSTAN_BUILTINS).map(|d| ("PHPStan", d)),
        Flavor::Nette => builtin(schema::NETTE_PARAMETERS).map(|d| ("Nette", d)),
        Flavor::Both => builtin(schema::NETTE_PARAMETERS)
            .map(|d| ("Nette", d))
            .or_else(|| builtin(schema::PHPSTAN_BUILTINS).map(|d| ("PHPStan", d))),
    };
    match doc_line {
        Some((by, d)) => format!("**%{name}%** — built-in parameter of {by}: {d}"),
        None => format!("**%{name}%** — not defined in this file or the files it includes"),
    }
}

fn service_hover(files: &mut Files, doc: Rc<Loaded>, name: &str) -> Option<String> {
    let found = definitions(files, doc.clone(), |file| file.model.service(name).cloned());
    let (file, d) = found.first()?;
    let class = d.class.as_deref().map(|c| format!(" · `{c}`")).unwrap_or_default();
    let body = excerpt(&file.parsed.text, d.item);
    Some(format!(
        "**@{name}**{class}\n\n```neon\n{body}\n```\n\nDefined in {}{}",
        where_defined(&doc, file, d.key),
        more(found.len() - 1)
    ))
}

fn class_hover(doc: &Loaded, fqn: &str) -> Option<String> {
    let project = openidea_plugin::host::project_of(&doc.path)?;
    let found = references::find_class(&project, fqn);
    let first = found.first()?;
    let root = project.root().unwrap_or_default();
    let path =
        first.location.path.strip_prefix(&root).map_or(first.location.path.as_str(), |p| p.trim_start_matches('/'));
    let Position { line, .. } = first.location.range.start;
    Some(format!("PHP {} `{fqn}` — `{path}`, line {}", first.kind, line + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::SymbolInfo;
    use openidea_plugin::mock::MockHost;

    fn doc(path: &str, text: &str) -> (Rc<Loaded>, usize) {
        let offset = text.find('|').unwrap();
        (Rc::new(Loaded::new(path.into(), text.replacen('|', "", 1))), offset)
    }

    #[test]
    fn includes_parameters_and_services_across_files() {
        let host = MockHost::install();
        host.add_file(
            "/project/config/local.neon",
            "includes:\n\t- common.neon\nparameters:\n\tdatabase:\n\t\tuser: root\nservices:\n\tdb: PDO\n",
        );
        host.add_file("/project/config/services.neon", "");
        let mut files = Files::default();
        let (d, at) = doc("/project/config/common.neon", "includes:\n\t- serv|ices.neon\n");
        assert_eq!(
            definition(&mut files, d, at),
            [Location { path: "/project/config/services.neon".into(), range: Range::default() }]
        );
        let (d, at) = doc("/project/config/common.neon", "includes:\n\t- missing|.neon\n");
        assert!(definition(&mut files, d, at).is_empty());

        let text = "includes:\n\t- local.neon\nservices:\n\t- Foo(%database.us|er%, @db)\n";
        let (d, at) = doc("/project/config/common.neon", text);
        let found = definition(&mut files, d, at);
        assert_eq!(found[0].path, "/project/config/local.neon");
        assert_eq!(found[0].range, Range::new(Position::new(4, 2), Position::new(4, 6)));

        let (d, at) = doc("/project/config/common.neon", &text.replace("us|er%, @db", "user%, @d|b"));
        assert_eq!(definition(&mut files, d, at)[0].range, Range::new(Position::new(6, 1), Position::new(6, 3)));
    }

    #[test]
    fn the_document_wins_and_others_are_listed_after_it() {
        let host = MockHost::install();
        host.add_file("/project/base.neon", "parameters:\n\tlevel: 1\n");
        let mut files = Files::default();
        let (d, at) = doc("/project/phpstan.neon", "includes:\n\t- base.neon\nparameters:\n\tlevel: 5\nx: %lev|el%\n");
        let found = definition(&mut files, d.clone(), at);
        assert_eq!(
            found.iter().map(|l| l.path.as_str()).collect::<Vec<_>>(),
            ["/project/phpstan.neon", "/project/base.neon"]
        );
        let h = hover(&mut files, d, at).unwrap();
        assert_eq!(h.markdown, "**%level%** = `5`\n\nDefined in `this file`, line 4 (overrides 1 more definition)");
        assert_eq!(h.range, Some(Range::new(Position::new(4, 3), Position::new(4, 10))));
    }

    #[test]
    fn classes_come_from_the_project_index() {
        let host = MockHost::install();
        let symbol = |name: &str, path: &str| SymbolInfo {
            name: name.into(),
            kind: "class".into(),
            container: None,
            location: Location { path: path.into(), range: Range::new(Position::new(4, 6), Position::new(4, 9)) },
        };
        host.set_symbols(
            "/project",
            vec![
                symbol("Foo", "/project/src/Other/Foo.php"),
                symbol("Foo", "/project/src/Model/Foo.php"),
                symbol("FooBar", "/project/src/Model/FooBar.php"),
            ],
        );
        let mut files = Files::default();
        let (d, at) = doc("/project/services.neon", "services:\n\t- App\\Model\\F|oo(@db)\n");
        let found = definition(&mut files, d.clone(), at);
        assert_eq!(found.iter().map(|l| l.path.as_str()).collect::<Vec<_>>(), ["/project/src/Model/Foo.php"]);
        let h = hover(&mut files, d, at).unwrap();
        assert_eq!(h.markdown, "PHP class `App\\Model\\Foo` — `src/Model/Foo.php`, line 5");
        // `@App\Model\Foo`: the service of that type, when no service is named so.
        let (d, at) = doc("/project/common.neon", "services:\n\t- Bar(@App\\Model\\F|oo)\n");
        assert_eq!(definition(&mut files, d.clone(), at)[0].path, "/project/src/Model/Foo.php");
        let h = hover(&mut files, d, at).unwrap();
        assert_eq!(h.markdown, "PHP class `App\\Model\\Foo` — `src/Model/Foo.php`, line 5");
        // Keys of PHPStan's `conditionalTags:` and Nette's `decorator:` are classes.
        for (text, classes) in [
            ("conditionalTags:\n\tApp\\Model\\F|oo:\n\t\tphpstan.rules.rule: %x%\n", 1),
            ("decorator:\n\tF|oo:\n\t\tinject: true\n", 2),
        ] {
            let (d, at) = doc("/project/phpstan.neon", text);
            assert_eq!(definition(&mut files, d.clone(), at).len(), classes, "{text:?}");
            assert!(hover(&mut files, d, at).unwrap().markdown.starts_with("PHP class"), "{text:?}");
        }
        let (d, at) = doc("/project/phpstan.neon", "parameters:\n\tF|oo: 1\n");
        assert!(definition(&mut files, d, at).is_empty(), "other keys are keys");
    }

    #[test]
    fn hovers_for_keys_services_and_builtins() {
        let _host = MockHost::install();
        let mut files = Files::default();
        let (d, at) = doc("/project/phpstan.neon", "parameters:\n\ttreatPhpDocTypes|AsCertain: false\n");
        let h = hover(&mut files, d, at).unwrap();
        assert!(h.markdown.starts_with("**treatPhpDocTypesAsCertain** — Trusts PHPDoc"), "{}", h.markdown);
        let (d, at) = doc("/project/phpstan.neon", "parameters:\n\ttmpDir: %currentWorking|Directory%/tmp\n");
        assert!(hover(&mut files, d, at).unwrap().markdown.contains("built-in parameter of PHPStan"));
        let (d, at) = doc("/project/common.neon", "services:\n\tdb:\n\t\tcreate: PDO(%dsn%)\n\t- Foo(@d|b)\n");
        let h = hover(&mut files, d, at).unwrap();
        assert!(
            h.markdown.starts_with("**@db** · `PDO`\n\n```neon\ndb:\n\t\tcreate: PDO(%dsn%)\n```"),
            "{}",
            h.markdown
        );
        let (d, at) = doc("/project/common.neon", "x: %nowh|ere%\n");
        assert!(hover(&mut files, d, at).unwrap().markdown.contains("not defined"));
        let (d, at) = doc("/project/common.neon", "x: just te|xt\n");
        assert!(hover(&mut files, d, at).is_none());
    }

    #[test]
    fn included_files_are_cached_until_they_change() {
        let host = MockHost::install();
        host.add_file("/project/b.neon", "parameters:\n\tx: 1\n");
        let mut files = Files::default();
        let (d, at) = doc("/project/a.neon", "includes:\n\t- b.neon\ny: %|x%\n");
        let first = files.related(d.clone());
        let again = files.related(d.clone());
        assert!(Rc::ptr_eq(&first[1], &again[1]), "unchanged: the same parse");
        host.add_file("/project/b.neon", "parameters:\n\tx: 2\n\tz: 3\n");
        let h = hover(&mut files, d, at).unwrap();
        assert!(h.markdown.starts_with("**%x%** = `2`"), "{}", h.markdown);
    }
}
