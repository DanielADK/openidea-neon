//! A PHPStan baseline of 5 MB parses in linear time. The budget is asserted in
//! release builds (`cargo test --release --test performance -- --nocapture`);
//! debug builds only print the timings.

use openidea_neon::neon::Parsed;
use openidea_neon::{NeonPlugin, highlight, outline};
use openidea_plugin::Position;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::HoverProvider;
use openidea_plugin::mock::MockHost;
use openidea_plugin::providers::{Document, Providers};
use openidea_plugin::syntax::SyntaxProvider;
use std::time::{Duration, Instant};

/// A baseline-like document of about `bytes` bytes.
pub fn baseline(bytes: usize) -> String {
    let mut text = String::from("parameters:\n\tignoreErrors:\n");
    let mut i = 0;
    while text.len() < bytes {
        let quoted = if i % 3 == 0 {
            format!(
                "'#^Parameter \\#1 \\$value{i} of method App\\\\Model\\\\Řada{i}\\:\\:get\\(\\) expects string, int given\\.$#'"
            )
        } else {
            format!("\"#^Call to an undefined method App\\\\\\\\Model\\\\\\\\Foo{i}\\\\:\\\\:bar\\\\(\\\\)\\\\.$#\"")
        };
        text.push_str(&format!(
            "\t\t-\n\t\t\tmessage: {quoted}\n\t\t\tidentifier: method.notFound\n\t\t\tcount: {}\n\t\t\tpath: src/Model/Foo{i}.php\n\n",
            i % 7 + 1
        ));
        i += 1;
    }
    text
}

fn best_of<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut out = None;
    for _ in 0..runs {
        let t = Instant::now();
        let r = f();
        best = best.min(t.elapsed());
        out = Some(r);
    }
    (best, out.expect("at least one run"))
}

#[test]
fn a_five_megabyte_baseline_parses_quickly() {
    let text = baseline(5 << 20);
    let (took, parsed) = best_of(5, || Parsed::new(text.clone()));
    assert!(parsed.doc.errors.is_empty(), "{:?}", &parsed.doc.errors[..parsed.doc.errors.len().min(3)]);
    eprintln!(
        "{} bytes, {} tokens, {} lines: scan + parse + line index in {took:?}",
        text.len(),
        parsed.lexed.tokens.len(),
        parsed.lines.line_count()
    );
    if !cfg!(debug_assertions) {
        assert!(took < Duration::from_millis(50), "{took:?}");
    }
}

#[test]
fn every_provider_answers_a_baseline_in_time() {
    let _host = MockHost::install();
    let text = baseline(5 << 20);
    let parsed = Parsed::new(text.clone());
    let (h, tokens) = best_of(5, || highlight::highlights(&parsed));
    let (o, symbols) = best_of(5, || outline::outline(&parsed));
    let (f, folds) = best_of(5, || outline::folding(&parsed));
    eprintln!(
        "highlights: {} tokens in {h:?}; outline: {} symbols in {o:?}; folding: {} folds in {f:?}",
        tokens.len(),
        symbols.len(),
        folds.len()
    );
    assert!(tokens.len() <= highlight::MAX_TOKENS);
    // The syntax provider asks three times per version: one parse serves all.
    let (all, _) = best_of(3, || {
        let mut plugin = NeonPlugin::default();
        (
            plugin.highlights("neon", &text).len(),
            plugin.outline("neon", &text).len(),
            plugin.folding("neon", &text).len(),
        )
    });
    eprintln!("syntax provider, first version: highlights + outline + folding in {all:?}");
    let doc = Document {
        path: "/project/phpstan-baseline.neon".into(),
        language: "neon".into(),
        version: 1,
        text: text.clone(),
    };
    // The editor providers take the parse the syntax provider made.
    let middle = Position::new(parsed.lines.line_count() as u32 / 2, 4);
    let mut plugin = NeonPlugin::default();
    plugin.highlights("neon", &text);
    let (first, _) = best_of(1, || plugin.completion(&doc, middle, None));
    eprintln!("first completion after the syntax provider: {first:?}");
    let (d, problems) = best_of(5, || plugin.diagnostics(&doc));
    let (c, _) = best_of(5, || plugin.completion(&doc, middle, None));
    let (v, _) = best_of(5, || plugin.hover(&doc, middle));
    eprintln!("diagnostics {d:?} ({} problems), completion {c:?}, hover {v:?} (parse cached)", problems.len());
    if !cfg!(debug_assertions) {
        for took in [h, o, f, first, d, c, v] {
            assert!(took < Duration::from_millis(50), "{took:?}");
        }
        assert!(all < Duration::from_millis(100), "{all:?}");
    }
}

#[test]
fn brackets_never_closed_take_linear_time() {
    let n = 40_000;
    for text in [format!("{}{}", "[".repeat(n), ")".repeat(n)), format!("a: {}\n{}", "(".repeat(n), "- ]\n".repeat(n))]
    {
        let (took, parsed) = best_of(3, || Parsed::new(text.clone()));
        assert!(!parsed.doc.errors.is_empty());
        eprintln!("{} bytes of brackets never closed: parsed in {took:?}", text.len());
        if !cfg!(debug_assertions) {
            assert!(took < Duration::from_millis(50), "{took:?}");
        }
    }
}
