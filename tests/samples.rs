//! Real configuration files must give no problem at all, not even a warning
//! about nette/neon 3. The fixtures are hand-written equivalents of
//! real-world Nette and PHPStan configs; set `NEON_SAMPLES_DIR` to a folder of
//! downloaded originals to check those too.

use openidea_neon::NeonPlugin;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::mock::MockHost;
use openidea_plugin::providers::Document;
use std::path::Path;

/// `values.neon` shows every construct of NEON, also the ones only nette/neon
/// 4.0.1 reads so (a key's colon and value on later lines, inside brackets).
const EXPECTED: &[(&str, &[&str])] = &[("values.neon", &["neon3-colon-on-next-line", "neon3-value-on-next-line"])];

fn check_dir(dir: &Path) -> usize {
    let _host = MockHost::install();
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    let mut checked = 0;
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if !(name.ends_with(".neon") || name.ends_with(".neon.dist")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // As the IDE asks: by the file's name, which decides the PHPStan warnings.
        let doc = Document { path: format!("/project/{name}"), language: "neon".into(), version: 1, text };
        let problems = NeonPlugin::default().diagnostics(&doc);
        let codes: Vec<&str> = problems.iter().filter_map(|d| d.code.as_deref()).collect();
        let expected = EXPECTED.iter().find(|(n, _)| *n == name).map_or(&[][..], |(_, codes)| *codes);
        assert_eq!(codes, expected, "{}: {:?}", path.display(), problems);
        checked += 1;
    }
    checked
}

#[test]
fn fixtures_have_no_problems() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    assert!(check_dir(&dir) >= 5);
}

#[test]
fn downloaded_samples_have_no_problems() {
    let Some(dir) = std::env::var_os("NEON_SAMPLES_DIR") else {
        eprintln!("NEON_SAMPLES_DIR is not set: skipping the downloaded samples");
        return;
    };
    let n = check_dir(Path::new(&dir));
    eprintln!("{n} downloaded samples have no problems");
}
