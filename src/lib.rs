//! NEON language support for OpenIDEA: the format of Nette and PHPStan
//! configuration. Highlighting, structure view and folding come from the
//! syntax provider; problems, completion, navigation and hovers from the
//! editor providers. Everything is computed from one parse per document
//! version, which is linear in the text.

pub mod completion;
pub mod config;
pub mod diagnostics;
pub mod highlight;
pub mod lines;
pub mod navigation;
pub mod neon;
pub mod outline;
pub mod references;
pub mod schema;
pub mod workspace;

use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::providers::{CompletionItem, Document, Providers};
use openidea_plugin::syntax::{Fold, Symbol, SyntaxProvider, Token};
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, register_plugin};
use std::rc::Rc;
use workspace::{Files, Loaded};

/// Documents parsed lately, by text: the syntax provider asks three times per
/// version and the editor providers once each, all for the same text.
const RECENT: usize = 4;
/// Text the kept parses hold together at most (a parse takes about five
/// times its text): a larger document is kept alone.
const RECENT_BYTES: usize = 8 << 20;

#[derive(Default)]
pub struct NeonPlugin {
    recent: Vec<Rc<Loaded>>,
    files: Files,
}

impl NeonPlugin {
    fn document(&mut self, path: &str, text: &str) -> Rc<Loaded> {
        if let Some(i) = self.recent.iter().position(|d| d.parsed.text == text) {
            let mut d = self.recent.remove(i);
            // One parse for every path (the syntax provider gives none).
            if !path.is_empty() && d.path != path {
                d = Rc::new(d.with_path(path));
            }
            self.recent.push(d.clone());
            return d;
        }
        // Room first: memory counts at its peak, while the new parse is built.
        let mut kept: usize = self.recent.iter().map(|d| d.parsed.text.len()).sum();
        while !self.recent.is_empty() && (self.recent.len() >= RECENT || kept + text.len() > RECENT_BYTES) {
            kept -= self.recent.remove(0).parsed.text.len();
        }
        let d = Rc::new(Loaded::new(path.to_owned(), text.to_owned()));
        self.recent.push(d.clone());
        d
    }

    fn at(&mut self, doc: &Document, at: Position) -> (Rc<Loaded>, usize) {
        let d = self.document(&doc.path, &doc.text);
        let offset = d.parsed.lines.offset(&d.parsed.text, at);
        (d, offset)
    }
}

impl Plugin for NeonPlugin {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

impl SyntaxProvider for NeonPlugin {
    fn highlights(&mut self, _language: &str, text: &str) -> Vec<Token> {
        highlight::highlights(&self.document("", text).parsed)
    }

    fn outline(&mut self, _language: &str, text: &str) -> Vec<Symbol> {
        outline::outline(&self.document("", text).parsed)
    }

    fn folding(&mut self, _language: &str, text: &str) -> Vec<Fold> {
        outline::folding(&self.document("", text).parsed)
    }
}

impl DiagnosticsProvider for NeonPlugin {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        let d = self.document(&doc.path, &doc.text);
        diagnostics::diagnostics(&d.parsed, &doc.path, schema::flavor(&doc.path, &d.model))
    }
}

impl Providers for NeonPlugin {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let (d, offset) = self.at(doc, at);
        completion::complete(&mut self.files, d, offset)
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let (d, offset) = self.at(doc, at);
        navigation::definition(&mut self.files, d, offset)
    }
}

impl HoverProvider for NeonPlugin {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        let (d, offset) = self.at(doc, at);
        navigation::hover(&mut self.files, d, offset)
    }
}

register_plugin!(NeonPlugin, syntax, providers, diagnostics, hover);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::syntax::SymbolKind;

    /// The IDE tries a file name's extensions from the longest (`neon.dist`
    /// of `phpstan.neon.dist`) to the shortest.
    #[test]
    fn the_language_claims_neon_and_neon_dist_files() {
        let definition = include_str!("../languages/neon.toml");
        let line = definition.lines().find(|l| l.starts_with("extensions")).unwrap();
        let extensions: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
        assert_eq!(extensions, ["neon", "neon.dist"]);
        let detects = |name: &str| {
            let lower = name.to_ascii_lowercase();
            let mut rest = lower.as_str();
            while let Some(dot) = rest.find('.') {
                rest = &rest[dot + 1..];
                if extensions.contains(&rest) {
                    return true;
                }
            }
            false
        };
        for name in [
            "common.neon",
            "phpstan.neon",
            "phpstan.neon.dist",
            "phpstan.dist.neon",
            "config.local.neon.dist",
            "A.NEON",
        ] {
            assert!(detects(name), "{name}");
        }
        for name in ["neon", "phpstan.dist", "x.neon.php"] {
            assert!(!detects(name), "{name}");
        }
    }

    #[test]
    fn the_editor_document_gets_problems() {
        let host = MockHost::install();
        host.open_editor("/project/config/common.neon", "neon", "a: 1\na: 2\n", &[]);
        let mut plugin = NeonPlugin::default();
        host.refresh_diagnostics(&mut plugin);
        let problems = &host.provider_diagnostics()["/project/config/common.neon"];
        assert_eq!(problems[0].message, "Duplicated key 'a'");
        host.type_in_editor("b: 3\n");
        host.refresh_diagnostics(&mut plugin);
        assert_eq!(host.provider_diagnostics()["/project/config/common.neon"].len(), 1);
    }

    #[test]
    fn one_parse_serves_the_syntax_calls_and_the_providers() {
        let _host = MockHost::install();
        let mut plugin = NeonPlugin::default();
        let text = "parameters:\n\tx: 1\ny: %x%\n";
        plugin.highlights("neon", text);
        let parsed = plugin.recent[0].parsed.clone();
        let doc = Document { path: "/project/a.neon".into(), language: "neon".into(), version: 1, text: text.into() };
        plugin.diagnostics(&doc);
        assert!(plugin.hover(&doc, Position::new(2, 4)).is_some());
        plugin.outline("neon", text);
        assert_eq!(plugin.recent.len(), 1);
        assert!(Rc::ptr_eq(&plugin.recent[0].parsed, &parsed), "parsed once");
        assert_eq!(plugin.recent[0].path, "/project/a.neon");
    }

    #[test]
    fn the_parses_kept_are_bounded_by_their_text() {
        let mut plugin = NeonPlugin::default();
        let text = |c: char, mib: usize| format!("a: {}\n", c.to_string().repeat(mib << 20));
        for c in ['a', 'b', 'c'] {
            plugin.highlights("neon", &text(c, 3));
        }
        assert_eq!(plugin.recent.len(), 2, "6 of 9 MiB");
        plugin.highlights("neon", &text('d', 9));
        assert_eq!(plugin.recent.len(), 1, "a large document alone");
        for c in "efghij".chars() {
            plugin.highlights("neon", &format!("{c}: 1\n"));
        }
        assert_eq!(plugin.recent.len(), RECENT);
    }

    #[test]
    fn a_valid_document_has_no_problems() {
        let host = MockHost::install();
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/phpstan.neon.dist")).unwrap();
        host.open_editor("/project/phpstan.neon.dist", "neon", &text, &[]);
        let mut plugin = NeonPlugin::default();
        host.refresh_diagnostics(&mut plugin);
        assert!(!host.provider_diagnostics().contains_key("/project/phpstan.neon.dist"));
    }

    #[test]
    fn non_ascii_before_tokens_keeps_positions_right() {
        let host = MockHost::install();
        let text = "parameters:\n\tčíslo😀: 1\nx: '😀 %čí'\ny: %číslo😀%\n";
        host.add_file("/project/a.neon", text);
        let mut plugin = NeonPlugin::default();
        let doc = Document { path: "/project/a.neon".into(), language: "neon".into(), version: 1, text: text.into() };
        // Line 3 `y: %číslo😀%`: the caret after `č` is UTF-16 column 5.
        let found = plugin.definition(&doc, Position::new(3, 5));
        assert_eq!(found[0].range, openidea_plugin::Range::new(Position::new(1, 1), Position::new(1, 8)));
        let items = plugin.completion(&doc, Position::new(2, 9), Some("%"));
        let item = items.iter().find(|i| i.label == "číslo😀").unwrap();
        assert_eq!(item.replace.map(|r| (r.start.character, r.end.character)), Some((8, 10)));
        let hover = plugin.hover(&doc, Position::new(3, 6)).unwrap();
        assert_eq!(hover.range, Some(openidea_plugin::Range::new(Position::new(3, 3), Position::new(3, 12))));
        // The syntax provider works in bytes.
        let symbols = plugin.outline("neon", text);
        assert_eq!(symbols[1].name, "číslo😀");
        assert_eq!(symbols[1].kind, SymbolKind::Property);
        assert_eq!(&text[symbols[1].name_range.clone()], "číslo😀");
    }
}
