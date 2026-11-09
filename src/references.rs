//! What the text at an offset refers to: an included file, a parameter, a
//! service or a PHP class.

use crate::config::{Step, path_at};
use crate::highlight::{is_class_name, percent_parts};
use crate::neon::{Parsed, Role, Span, TokenKind};
use openidea_plugin::{Project, SymbolInfo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reference {
    /// An `includes:` entry, as written.
    Include(String),
    /// `%name%`: the name.
    Parameter(String),
    /// `@name`: the name.
    Service(String),
    /// A PHP class, interface, trait or enum.
    Class(String),
    /// A key the schema may know.
    Key { path: Vec<Step>, name: String },
}

/// Keys whose values name classes.
const CLASS_KEYS: &[&str] = &["create", "factory", "class", "type", "implement", "extends", "implements", "excluded"];

/// The reference at `offset`, and the text it covers.
pub fn at(p: &Parsed, offset: usize) -> Option<(Reference, Span)> {
    let i = p.token_at(offset)?;
    let t = p.lexed.tokens[i];
    if !matches!(t.kind, TokenKind::Literal | TokenKind::String) {
        return None;
    }
    let raw = t.span.text(&p.text);
    let base = t.span.start as usize;
    let role = p.doc.role(i);
    let path = path_at(p, offset);
    if role == Role::Key {
        if keyed_by_class(&path)
            && let Some((fqn, span)) = class_at(p, i, offset, &path)
        {
            return Some((Reference::Class(fqn), span));
        }
        return Some((Reference::Key { path, name: p.key_name(i as u32).into_owned() }, t.span));
    }
    if matches!(path.as_slice(), [Step::Key(k), Step::Item] if k == "includes") {
        return Some((Reference::Include(p.scalar_text(i as u32).into_owned()), t.span));
    }
    for (start, end, escape) in percent_parts(raw) {
        if !escape && base + start <= offset && offset <= base + end {
            let span = Span::new(base + start, base + end);
            return Some((Reference::Parameter(raw[start + 1..end - 1].to_owned()), span));
        }
    }
    if t.kind == TokenKind::Literal
        && let Some(name) = raw.strip_prefix('@')
    {
        let name_end = name.find("::").unwrap_or(name.len());
        if offset <= base + 1 + name_end {
            let span = Span::new(base, base + 1 + name_end);
            return Some((Reference::Service(name[..name_end].to_owned()), span));
        }
    }
    class_at(p, i, offset, &path).map(|(fqn, span)| (Reference::Class(fqn), span))
}

/// Sections keyed by classes: PHPStan's `conditionalTags`, Nette's `decorator`.
fn keyed_by_class(path: &[Step]) -> bool {
    matches!(path, [Step::Key(k)] if k == "conditionalTags" || k == "decorator")
}

/// A class name around `offset` in token `i`.
fn class_at(p: &Parsed, i: usize, offset: usize, path: &[Step]) -> Option<(String, Span)> {
    let t = p.lexed.tokens[i];
    let raw = t.span.text(&p.text);
    let base = t.span.start as usize;
    let rel = offset.checked_sub(base)?.min(raw.len());
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '\\' || !c.is_ascii();
    let start =
        raw[..rel].rfind(|c: char| !is_name(c)).map_or(0, |k| k + raw[k..].chars().next().map_or(1, char::len_utf8));
    let end = raw[rel..].find(|c: char| !is_name(c)).map_or(raw.len(), |k| rel + k);
    let word = raw.get(start..end)?;
    if !is_class_name(word) {
        return None;
    }
    let before = &raw[..start];
    let after = &raw[end..];
    // A member after `::` is not the class.
    if before.ends_with("::") {
        return None;
    }
    let upper = word.trim_start_matches('\\').starts_with(|c: char| c.is_ascii_uppercase());
    let in_services =
        matches!(path.first(), Some(Step::Key(k)) if k == "services" || k == "rules" || k == "extensions");
    let class_key = matches!(path.last(), Some(Step::Key(k)) if CLASS_KEYS.contains(&k.as_str()));
    let head = p.doc.role(i) == Role::EntityHead;
    let class_named = p.doc.role(i) == Role::Key && keyed_by_class(path);
    let likely = word.contains('\\')
        || after.starts_with("::")
        || class_named
        || (upper && (head || class_key || in_services || before == "@"));
    likely.then(|| (word.trim_start_matches('\\').to_owned(), Span::new(base + start, base + end)))
}

/// Classes of the project index named like `fqn`, the ones whose file path
/// matches its namespace best first (PSR-4).
pub fn find_class(project: &Project, fqn: &str) -> Vec<SymbolInfo> {
    let fqn = fqn.trim_start_matches('\\');
    let short = fqn.rsplit('\\').next().unwrap_or(fqn);
    if short.is_empty() || short.len() > 256 {
        return Vec::new();
    }
    let Ok(symbols) = project.symbols(short, 200) else { return Vec::new() };
    let namespace: Vec<String> = fqn.split('\\').rev().skip(1).map(str::to_ascii_lowercase).collect();
    let ns = fqn[..fqn.len() - short.len()].trim_end_matches('\\');
    let mut scored: Vec<(usize, SymbolInfo)> = symbols
        .into_iter()
        .filter(|s| {
            matches!(s.kind.as_str(), "class" | "interface" | "trait" | "enum") && s.name.eq_ignore_ascii_case(short)
        })
        .map(|s| {
            let dirs: Vec<String> = s.location.path.rsplit(['/', '\\']).skip(1).map(str::to_ascii_lowercase).collect();
            let matched = namespace.iter().zip(&dirs).take_while(|(a, b)| a == b).count();
            let container = !ns.is_empty()
                && s.container.as_deref().is_some_and(|c| c.trim_start_matches('\\').eq_ignore_ascii_case(ns));
            let exact = usize::from(s.name == short);
            (matched * 4 + usize::from(container) * 64 + exact, s)
        })
        .collect();
    let Some(best) = scored.iter().map(|(score, _)| *score).max() else { return Vec::new() };
    scored.retain(|(score, _)| *score == best);
    scored.into_iter().map(|(_, s)| s).take(10).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(text: &str) -> Option<Reference> {
        let offset = text.find('|').unwrap();
        let text = text.replacen('|', "", 1);
        at(&Parsed::new(text), offset).map(|(r, _)| r)
    }

    #[test]
    fn references_by_place() {
        assert_eq!(reference("includes:\n\t- serv|ices.neon"), Some(Reference::Include("services.neon".into())));
        assert_eq!(reference("a: %mail|er.host%/x"), Some(Reference::Parameter("mailer.host".into())));
        assert_eq!(reference("a: '%temp|Dir%/cache'"), Some(Reference::Parameter("tempDir".into())));
        assert_eq!(reference("a: '100%|% done'"), None);
        assert_eq!(reference("a: @da|tabase::query()"), Some(Reference::Service("database".into())));
        assert_eq!(reference("- App\\Model\\Fo|o(@db)"), Some(Reference::Class("App\\Model\\Foo".into())));
        assert_eq!(reference("- Fo|o(@db)"), Some(Reference::Class("Foo".into())));
        assert_eq!(reference("a: PD|O::ATTR_ERRMODE"), Some(Reference::Class("PDO".into())));
        assert_eq!(reference("a: PDO::ATTR_ERR|MODE"), None);
        assert_eq!(reference("a: just a str|ing"), None);
        assert_eq!(
            reference("services:\n\tx:\n\t\ttype: Nette\\Rout|er"),
            Some(Reference::Class("Nette\\Router".into()))
        );
        assert_eq!(reference("services:\n\t- @App\\Fo|o::create"), Some(Reference::Service("App\\Foo".into())));
        assert!(matches!(reference("parameters:\n\tle|vel: 5"), Some(Reference::Key { name, .. }) if name == "level"));
    }
}
