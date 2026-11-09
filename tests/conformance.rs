//! The conformance corpus of the NEON specification
//! (https://github.com/nette/neon-specification, MIT): set `NEON_SPEC_DIR` to
//! a checkout to run it. Every `decode/` case must parse without problems,
//! every `errors/` case must fail at nette/neon's position, with its message
//! except for a bracket not closed at the end of the input, where nette/neon
//! says `Unexpected end` and this parser which bracket.

use openidea_neon::lines::LineIndex;
use openidea_neon::neon::Parsed;
use std::path::Path;

fn cases(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == ext)).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// `message` and `line:column` (1-based, code points), as the corpus writes them.
fn first_error(text: &str) -> Option<(String, String)> {
    let parsed = Parsed::new(text.to_owned());
    let e = parsed.doc.errors.iter().min_by_key(|e| e.at)?;
    let lines = LineIndex::new(text);
    let line = lines.line_of(e.at as usize);
    let start = lines.line_start(line) as usize;
    let column = text[start..e.at as usize].chars().count() + 1;
    Some((e.message.clone(), format!("{}:{column}", line + 1)))
}

#[test]
fn corpus() {
    let Some(root) = std::env::var_os("NEON_SPEC_DIR") else {
        eprintln!("NEON_SPEC_DIR is not set: skipping the conformance corpus");
        return;
    };
    let corpus = Path::new(&root).join("corpus");
    let mut failures = Vec::new();
    let mut decode = cases(&corpus.join("decode"), "neon");
    decode.extend(cases(&corpus.join("roundtrip"), "neon"));
    for case in &decode {
        let Ok(text) = std::fs::read_to_string(case) else { continue }; // not UTF-8: the host never sends it
        if let Some(e) = first_error(&text) {
            failures.push(format!("{}: unexpected {e:?}", case.display()));
        }
    }
    let errors = cases(&corpus.join("errors"), "neon");
    let (mut exact, mut unclosed, mut utf8) = (0, 0, 0);
    let mut differ = Vec::new();
    for case in &errors {
        let Ok(text) = std::fs::read_to_string(case) else {
            utf8 += 1;
            continue;
        };
        let expected = std::fs::read_to_string(case.with_extension("error")).unwrap_or_default();
        let mut expected = expected.lines();
        let (message, at) = (expected.next().unwrap_or(""), expected.next().unwrap_or(""));
        match first_error(&text) {
            None => failures.push(format!("{}: accepted, expected {message} at {at}", case.display())),
            Some((m, a)) if m == message && a == at => exact += 1,
            Some((m, a)) if message == "Unexpected end" && m.starts_with("Unexpected end: '") && a == at => {
                unclosed += 1;
                eprintln!("  {}: {m} (nette/neon: {message}) at {a}", case.display());
            }
            Some((m, a)) => differ.push(format!("{}: {m} at {a}, nette: {message} at {at}", case.display())),
        }
    }
    eprintln!(
        "decode: {} cases; errors: {} cases, {exact} with nette/neon's message and position, {unclosed} at its \
         position naming the bracket not closed, {} differently, {utf8} not UTF-8",
        decode.len(),
        errors.len(),
        differ.len()
    );
    for d in &differ {
        eprintln!("  {d}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(differ.is_empty(), "{} cases differ from nette/neon", differ.len());
}
