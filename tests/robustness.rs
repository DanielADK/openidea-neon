//! The plugin runs with `panic = "abort"`: a panic would end its instance.
//! Every provider is run over thousands of broken variants of real documents
//! (characters deleted, inserted, lines cut) and must answer sensibly.

use openidea_neon::NeonPlugin;
use openidea_neon::neon::Parsed;
use openidea_neon::{highlight, outline};
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::HoverProvider;
use openidea_plugin::mock::MockHost;
use openidea_plugin::providers::{Document, Providers};
use openidea_plugin::syntax::SyntaxProvider;

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

const PIECES: &[&str] = &[
    "\n", "\t", " ", ":", "-", ",", "=", "[", "]", "{", "}", "(", ")", "'", "\"", "#", "@", "%", "\\", "'''\n",
    "\"\"\"\n", "\r\n", "\r", "`", "č", "😀", "::", "- ", ": ", "\n\t\t", "%a.b%", "@x::y()", "\u{feff}",
];

fn boundaries(text: &str) -> Vec<usize> {
    (0..=text.len()).filter(|&i| text.is_char_boundary(i)).collect()
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let cuts = boundaries(text);
    let a = cuts[rng.below(cuts.len())];
    let b = cuts[rng.below(cuts.len())];
    let (a, b) = (a.min(b), a.max(b));
    match rng.below(4) {
        0 => {
            let to = cuts.iter().copied().find(|&c| c >= a + (b - a).min(8)).unwrap_or(text.len());
            format!("{}{}", &text[..a], &text[to..])
        }
        1 => format!("{}{}{}", &text[..a], PIECES[rng.below(PIECES.len())], &text[a..]),
        2 => text[..a].to_owned(),
        _ => format!("{}{}{}", &text[..a], PIECES[rng.below(PIECES.len())], &text[b..]),
    }
}

fn check(plugin: &mut NeonPlugin, text: &str, rng: &mut Rng) {
    let p = Parsed::new(text.to_owned());
    let mut end = 0;
    for t in highlight::highlights(&p) {
        assert!(t.range.start >= end && t.range.end > t.range.start && t.range.end <= text.len(), "{text:?}");
        assert!(text.is_char_boundary(t.range.start) && text.is_char_boundary(t.range.end), "{text:?}");
        end = t.range.end;
    }
    for (i, s) in outline::outline(&p).iter().enumerate() {
        assert!(s.parent.is_none_or(|parent| parent < i), "{text:?}");
        assert!(s.range.start <= s.range.end && s.range.end <= text.len(), "{text:?}");
    }
    let lines = p.lines.line_count() as u32;
    for f in outline::folding(&p) {
        assert!(f.start_line < f.end_line && f.end_line < lines, "{text:?}");
    }
    let doc =
        Document { path: "/project/config/common.neon".into(), language: "neon".into(), version: 1, text: text.into() };
    for d in plugin.diagnostics(&doc) {
        assert!(d.range.start <= d.range.end && d.range.end.line < lines, "{text:?}");
    }
    let _ = plugin.outline("neon", text);
    for _ in 0..4 {
        let line = rng.below(lines as usize + 1) as u32;
        let at = openidea_plugin::Position::new(line, rng.below(30) as u32);
        let _ = plugin.completion(&doc, at, None);
        let _ = plugin.definition(&doc, at);
        let _ = plugin.hover(&doc, at);
    }
}

#[test]
fn broken_documents_never_panic() {
    let host = MockHost::install();
    host.add_file("/project/config/services.neon", "services:\n\tdb: PDO\n");
    host.add_file("/project/config/local.neon", "includes:\n\t- common.neon\nparameters:\n\ta: 1\n");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut inputs: Vec<String> =
        std::fs::read_dir(dir).unwrap().flatten().map(|e| std::fs::read_to_string(e.path()).unwrap()).collect();
    inputs.extend(
        [
            "",
            "-",
            "- - - -",
            "a:\n\t- b: 1\n\t  c:\n\t\t\t- d",
            "{a: [b: (c: {",
            "'''\n",
            "x: \"\\u",
            "\u{feff}\t a:\n\t \tb:",
            "a: 1\n  b: 2\n c: 3\n\td: 4",
        ]
        .map(str::to_owned),
    );
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut plugin = NeonPlugin::default();
    let rounds = if cfg!(debug_assertions) { 150 } else { 1500 };
    for input in &inputs {
        check(&mut plugin, input, &mut rng);
        let mut text = input.clone();
        for round in 0..rounds {
            text = mutate(&mut rng, if round % 10 == 0 { input } else { &text });
            check(&mut plugin, &text, &mut rng);
        }
    }
}

#[test]
fn deep_nesting_stays_within_the_stack() {
    let _host = MockHost::install();
    let mut plugin = NeonPlugin::default();
    let mut rng = Rng(42);
    for text in [
        "[".repeat(100_000),
        "- ".repeat(5_000),
        format!("a: {}", "Foo(".repeat(2_000)),
        (0..700).map(|i| format!("{}k{i}:\n", "\t".repeat(i))).collect(),
        "{a: ".repeat(3_000),
        format!("{}\n{}", "[".repeat(20_000), "\tx\n".repeat(20_000)),
    ] {
        check(&mut plugin, &text, &mut rng);
    }
}
