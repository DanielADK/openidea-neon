//! Problems while typing: what nette/neon would refuse, all of them at once,
//! and as warnings what nette/neon 3 reads otherwise.

use crate::neon::{ErrorKind, Neon3, Neon3Note, Parsed, SyntaxError};
use crate::schema::Flavor;
use openidea_plugin::{Diagnostic, Location, RelatedInformation, Severity};
use std::collections::HashSet;

/// Problems of one document at most (the host keeps 5,000).
const MAX_PROBLEMS: usize = 1000;

/// The problems of a document. PHPStan bundles nette/neon 3.3, so in its
/// files what only 3.3 reads otherwise is shown as well.
pub fn diagnostics(p: &Parsed, path: &str, flavor: Flavor) -> Vec<Diagnostic> {
    let phpstan = flavor == Flavor::PhpStan;
    let mut found: Vec<(u32, u32, Diagnostic)> =
        p.doc.errors.iter().map(|e| (e.span.start, e.span.end, error(p, path, e, phpstan))).collect();
    let errors: HashSet<u32> = p.doc.errors.iter().map(|e| e.span.start).collect();
    found.extend(
        p.doc
            .neon3
            .iter()
            .filter(|n| (phpstan || !n.kind.only_3_3()) && !errors.contains(&n.span.start))
            .map(|n| (n.span.start, n.span.end, neon3(p, n))),
    );
    found.sort_by_key(|(start, end, _)| (*start, *end));
    found.into_iter().take(MAX_PROBLEMS).map(|(_, _, d)| d).collect()
}

fn error(p: &Parsed, path: &str, e: &SyntaxError, phpstan: bool) -> Diagnostic {
    let range = p.lines.range(&p.text, e.span.start as usize, e.span.end as usize);
    let mut d = if e.kind == ErrorKind::HexEscape && phpstan {
        Diagnostic::new(
            range,
            Severity::Warning,
            "PHPStan's nette/neon 3.3 reads \\xHH (deprecated), nette/neon 3.4 and later refuse it: write \\u00HH",
        )
    } else {
        Diagnostic::new(range, Severity::Error, e.message.clone())
    }
    .code(e.kind.code());
    if let Some((span, message)) = e.related {
        d.related.push(RelatedInformation {
            location: Location {
                path: path.to_owned(),
                range: p.lines.range(&p.text, span.start as usize, span.end as usize),
            },
            message: message.to_owned(),
        });
    }
    d
}

fn neon3(p: &Parsed, n: &Neon3Note) -> Diagnostic {
    let text = n.span.text(&p.text);
    let message = match n.kind {
        Neon3::ValueOnNextLine => {
            let key = n.key.map(|k| p.key_name(k)).unwrap_or_default();
            format!(
                "nette/neon 3 (PHPStan, Nette 3) reads key '{key}' as null and this as an item of its own; \
                 only since 4.0.1 is it the key's value"
            )
        }
        Neon3::ColonOnNextLine => {
            format!("nette/neon 3 (PHPStan, Nette 3) refuses '{text}' on a later line than its key")
        }
        Neon3::CommaOnNextLine => "PHPStan's nette/neon 3.3 refuses a comma at the start of a line".into(),
        Neon3::TabAfterDash => "PHPStan's nette/neon 3.3 refuses this tab: it indents the lines of a block \
                                starting on a '-' line by two spaces (Invalid combination of tabs and spaces)"
            .into(),
        Neon3::OnOff => {
            let value = if text.eq_ignore_ascii_case("on") { "true" } else { "false" };
            format!(
                "PHPStan's nette/neon 3.3 reads '{text}' as {value} (deprecated), nette/neon 3.4 and later as \
                 the string '{text}': write {value}"
            )
        }
        Neon3::ByteOrderMark => "nette/neon 3 (PHPStan, Nette 3) reads the byte order mark as part of the first \
                                 key or value: save the file without it"
            .into(),
    };
    let range = p.lines.range(&p.text, n.span.start as usize, n.span.end as usize);
    Diagnostic::new(range, Severity::Warning, message).code(n.kind.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{Position, Range};

    fn problems(text: &str, flavor: Flavor) -> Vec<(u32, u32, Severity, String)> {
        let p = Parsed::new(text.into());
        diagnostics(&p, "/p/a.neon", flavor)
            .into_iter()
            .map(|d| (d.range.start.line, d.range.start.character, d.severity, d.code.unwrap_or_default()))
            .collect()
    }

    #[test]
    fn ranges_are_utf16_and_duplicates_point_to_the_first_key() {
        let text = "č😀: 1\nč😀: 2\n";
        let p = Parsed::new(text.into());
        let d = diagnostics(&p, "/project/a.neon", Flavor::Both);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "Duplicated key 'č😀'");
        assert_eq!(d[0].code.as_deref(), Some("duplicate-key"));
        assert_eq!(d[0].range, Range::new(Position::new(1, 0), Position::new(1, 3)));
        assert_eq!(d[0].related[0].location.range, Range::new(Position::new(0, 0), Position::new(0, 3)));
    }

    #[test]
    fn several_problems_are_reported_in_order() {
        let text = "a: [1\nb: `x\nc:\n  d: 1\n e: 2\n";
        let p = Parsed::new(text.into());
        let found = diagnostics(&p, "/p.neon", Flavor::Both);
        let lines: Vec<(u32, &str)> =
            found.iter().map(|d| (d.range.start.line, d.code.as_deref().unwrap_or(""))).collect();
        assert_eq!(lines, [(0, "unclosed"), (1, "syntax"), (4, "bad-indentation")]);
    }

    #[test]
    fn what_nette_neon_3_reads_otherwise_is_a_warning() {
        use Severity::*;
        let w = |line, col, code: &str| (line, col, Warning, code.to_owned());
        // In every file: what all of nette/neon 3 reads otherwise.
        assert_eq!(problems("x: {a:\n\tb}\n", Flavor::Nette), [w(1, 1, "neon3-value-on-next-line")]);
        assert_eq!(problems("x: {a\n: b}\n", Flavor::Nette), [w(1, 0, "neon3-colon-on-next-line")]);
        assert_eq!(problems("\u{feff}a: 1\n", Flavor::Nette), [w(0, 0, "neon3-byte-order-mark")]);
        assert_eq!(problems("\u{feff}# only a comment\n", Flavor::Nette), []);
        // Only in PHPStan's files: what only nette/neon 3.3 reads otherwise.
        let only_3_3 = [
            ("x: [a\n\t, b]\n", w(1, 1, "neon3-comma-on-next-line")),
            ("x:\n\t- a: 1\n\t\tb: 2\n", w(2, 2, "neon3-tab-after-dash")),
            ("x: off\ny: {On: ON}\n", w(0, 3, "neon3-on-off")),
        ];
        for (text, warning) in only_3_3 {
            assert_eq!(problems(text, Flavor::Nette), [], "{text:?}");
            assert_eq!(problems(text, Flavor::PhpStan)[0], warning, "{text:?}");
        }
        assert_eq!(problems("y: {On: ON}\n", Flavor::PhpStan), [w(0, 8, "neon3-on-off")], "keys are no booleans");
        let message = Parsed::new("x: {a:\n\tb}\n".into());
        let d = &diagnostics(&message, "/p.neon", Flavor::Nette)[0];
        assert!(d.message.starts_with("nette/neon 3 (PHPStan, Nette 3) reads key 'a' as null"), "{}", d.message);
        // `\xHH`: refused since nette/neon 3.4, read by PHPStan's 3.3.
        let hex = "x: \"a\\x41b\"\n";
        assert_eq!(problems(hex, Flavor::Nette), [(0, 3, Error, "invalid-escape".into())]);
        assert_eq!(problems(hex, Flavor::PhpStan), [(0, 3, Warning, "invalid-escape".into())]);
        assert_eq!(problems("x: \"\\x41\\q\"\n", Flavor::PhpStan)[0].2, Error, "3.3 refuses \\q");
        // A problem nette/neon 4.0.1 has at the same place wins.
        assert_eq!(problems("- a: 1\n\tb: 2\n\tc\n", Flavor::PhpStan), [(1, 1, Error, "tabs-and-spaces".into())]);
    }
}
