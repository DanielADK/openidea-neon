//! Configuration across files: a document with the files it includes,
//! transitively, read through the project and cached by path, size and
//! modification time. Bounded and cycle-safe.

use crate::config::Model;
use crate::neon::Parsed;
use openidea_plugin::{FileKind, FileStat, Project};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Files followed from one document at most.
const MAX_FILES: usize = 32;
/// How deep includes are followed.
const MAX_DEPTH: usize = 8;
/// Larger included files (a big baseline) are not read on the editing path.
pub const MAX_INCLUDED_BYTES: u64 = 2 << 20;
/// Included files kept parsed: at most this many, holding at most this much
/// text together (a parse takes about five times its text).
const MAX_CACHED: usize = 64;
const MAX_CACHED_BYTES: usize = 8 << 20;
/// Text read and parsed for one provider call at most: files beyond it are
/// read by the next calls, so that no call runs long.
const MAX_READ_PER_CALL: u64 = 4 << 20;

/// A parsed file and what it defines. The parse is shared by every path its
/// text is asked for under.
pub struct Loaded {
    pub path: String,
    pub parsed: Rc<Parsed>,
    pub model: Rc<Model>,
}

impl Loaded {
    pub fn new(path: String, text: String) -> Self {
        let parsed = Parsed::new(text);
        let model = Model::new(&parsed);
        Loaded { path, parsed: Rc::new(parsed), model: Rc::new(model) }
    }

    /// The same parse under another path.
    pub fn with_path(&self, path: &str) -> Self {
        Loaded { path: path.to_owned(), parsed: self.parsed.clone(), model: self.model.clone() }
    }
}

struct Cached {
    stat: FileStat,
    loaded: Rc<Loaded>,
    /// The last use, to drop the least recently used file first.
    used: u64,
}

#[derive(Default)]
pub struct Files {
    cache: HashMap<String, Cached>,
    clock: u64,
}

impl Files {
    /// The file at an absolute path inside the project, parsed. `None` when it
    /// can't be read (missing, too large, not granted) or when parsing it
    /// would take more than what is left of `budget`.
    fn load(&mut self, project: &Project, path: &str, budget: &mut u64) -> Option<Rc<Loaded>> {
        let stat = project.stat(path).ok()?;
        if stat.kind != FileKind::File || stat.size > MAX_INCLUDED_BYTES {
            return None;
        }
        self.clock += 1;
        match self.cache.get_mut(path) {
            Some(c) if c.stat.size == stat.size && c.stat.modified.is_some() && c.stat.modified == stat.modified => {
                c.used = self.clock;
                return Some(c.loaded.clone());
            }
            // A file not known yet must fit before it is read at all.
            None if stat.size > *budget => return None,
            _ => {}
        }
        let text = project.read_file(path).ok()?;
        // Without a modification time, the text decides.
        let loaded = match self.cache.get(path) {
            Some(c) if c.loaded.parsed.text == text => c.loaded.clone(),
            _ => {
                *budget = budget.checked_sub(text.len() as u64)?;
                Rc::new(Loaded::new(path.to_owned(), text))
            }
        };
        self.cache.insert(path.to_owned(), Cached { stat, loaded: loaded.clone(), used: self.clock });
        self.evict();
        Some(loaded)
    }

    /// Drops the least recently used files beyond the limits.
    fn evict(&mut self) {
        let mut bytes: usize = self.cache.values().map(|c| c.loaded.parsed.text.len()).sum();
        while self.cache.len() > MAX_CACHED || bytes > MAX_CACHED_BYTES {
            let Some(oldest) = self.cache.iter().min_by_key(|(_, c)| c.used).map(|(path, _)| path.clone()) else {
                break;
            };
            if let Some(dropped) = self.cache.remove(&oldest) {
                bytes -= dropped.loaded.parsed.text.len();
            }
        }
    }

    /// The document and the files it includes, in the order their definitions
    /// win: the document first, then its includes from the last one (Nette
    /// merges them in order, so later ones override earlier ones), each
    /// followed by what it includes. A file is read only when its turn comes.
    pub fn related(&mut self, current: Rc<Loaded>) -> Vec<Rc<Loaded>> {
        let Some(project) = openidea_plugin::host::project_of(&current.path) else { return vec![current] };
        let mut budget = MAX_READ_PER_CALL;
        let mut seen: HashSet<String> = HashSet::from([normalize(&current.path)]);
        // Depth first; an include's children are pushed in order, so the last is visited first.
        let mut stack: Vec<(String, usize)> = Vec::new();
        push_includes(&current, 1, &mut seen, &mut stack);
        let mut out = vec![current];
        while out.len() < MAX_FILES
            && let Some((path, depth)) = stack.pop()
        {
            let Some(file) = self.load(&project, &path, &mut budget) else { continue };
            if depth < MAX_DEPTH {
                push_includes(&file, depth + 1, &mut seen, &mut stack);
            }
            out.push(file);
        }
        out
    }
}

fn push_includes(file: &Loaded, depth: usize, seen: &mut HashSet<String>, stack: &mut Vec<(String, usize)>) {
    for (entry, _) in &file.model.includes {
        if let Some(path) = include_path(&file.path, entry)
            && seen.insert(path.clone())
        {
            stack.push((path, depth));
        }
    }
}

/// The absolute path of an `includes:` entry of the file at `from`, if it is
/// a NEON file this plugin can follow (not `phar://`, not built from parameters).
pub fn include_path(from: &str, entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() || entry.contains('%') || entry.contains("://") {
        return None;
    }
    let name = entry.rsplit(['/', '\\']).next().unwrap_or(entry).to_ascii_lowercase();
    if !(name.ends_with(".neon") || name.ends_with(".neon.dist")) {
        return None;
    }
    Some(join(dir_of(from), entry))
}

pub fn dir_of(path: &str) -> &str {
    path.rfind(['/', '\\']).map_or("", |i| &path[..i])
}

fn is_windows(path: &str) -> bool {
    path.as_bytes().get(1) == Some(&b':')
}

/// `rel` against `dir`, normalized; an absolute `rel` stays.
pub fn join(dir: &str, rel: &str) -> String {
    let windows = is_windows(dir) || is_windows(rel);
    let absolute = rel.starts_with('/') || rel.starts_with('\\') || is_windows(rel);
    let joined = if absolute { rel.to_owned() } else { format!("{dir}/{rel}") };
    normalize(&if windows { joined.replace('\\', "/") } else { joined })
}

/// `.`, `..` and repeated separators removed.
pub fn normalize(path: &str) -> String {
    let (prefix, rest) = if let Some(rest) = path.strip_prefix('/') {
        ("/".to_owned(), rest)
    } else if is_windows(path) {
        (format!("{}/", &path[..2]), path[2..].trim_start_matches('/'))
    } else {
        (String::new(), path)
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    prefix + &parts.join("/")
}

/// `to` written relative to the directory `from`.
pub fn relative(from: &str, to: &str) -> String {
    let a: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn paths() {
        assert_eq!(join("/p/config", "../phpstan-baseline.neon"), "/p/phpstan-baseline.neon");
        assert_eq!(join("/p/config", "./a//b.neon"), "/p/config/a/b.neon");
        assert_eq!(join("/p", "/abs/x.neon"), "/abs/x.neon");
        assert_eq!(join("C:\\p\\config", "..\\x.neon"), "C:/p/x.neon");
        assert_eq!(include_path("/p/a.neon", "b.neon"), Some("/p/b.neon".into()));
        assert_eq!(include_path("/p/a.neon", "phpstan.neon.dist"), Some("/p/phpstan.neon.dist".into()));
        assert_eq!(include_path("/p/a.neon", "phar://phpstan.phar/conf/bleedingEdge.neon"), None);
        assert_eq!(include_path("/p/a.neon", "%rootDir%/conf/x.neon"), None);
        assert_eq!(include_path("/p/a.neon", "parameters.php"), None);
        assert_eq!(relative("/p/config", "/p/config/services.neon"), "services.neon");
        assert_eq!(relative("/p/config", "/p/phpstan.neon"), "../phpstan.neon");
    }

    #[test]
    fn includes_are_followed_in_priority_order_without_cycles() {
        let host = MockHost::install();
        host.add_file("/project/a.neon", "includes:\n\t- b.neon\n\t- c.neon\n");
        host.add_file("/project/b.neon", "includes:\n\t- d.neon\n\t- a.neon\n");
        host.add_file("/project/c.neon", "includes:\n\t- missing.neon\n");
        host.add_file("/project/d.neon", "includes:\n\t- b.neon\n");
        let mut files = Files::default();
        let current = Rc::new(Loaded::new("/project/a.neon".into(), "includes:\n\t- b.neon\n\t- c.neon\n".into()));
        let order: Vec<String> = files.related(current).iter().map(|l| l.path.clone()).collect();
        assert_eq!(order, ["/project/a.neon", "/project/c.neon", "/project/b.neon", "/project/d.neon"]);
    }

    #[test]
    fn only_the_files_followed_are_read() {
        let host = MockHost::install();
        let mut includes = String::from("includes:\n");
        for i in 0..200 {
            host.add_file(&format!("/project/inc/f{i}.neon"), &format!("parameters:\n\tp{i}: {i}\n"));
            includes.push_str(&format!("\t- inc/f{i}.neon\n"));
        }
        let mut files = Files::default();
        let current = Rc::new(Loaded::new("/project/a.neon".into(), includes));
        let related = files.related(current.clone());
        assert_eq!(related.len(), MAX_FILES);
        assert_eq!(related[1].path, "/project/inc/f199.neon", "the last include first");
        assert_eq!(files.cache.len(), MAX_FILES - 1, "the other includes are not read");
        let again = files.related(current);
        assert!(again.iter().zip(&related).all(|(a, b)| Rc::ptr_eq(a, b)), "and the parses are kept");
    }

    #[test]
    fn the_cache_drops_the_least_recently_used_file() {
        let host = MockHost::install();
        let project = openidea_plugin::host::project_of("/project/a.neon").unwrap();
        let mut files = Files::default();
        let load = |files: &mut Files, i: usize| {
            let path = format!("/project/f{i}.neon");
            host.add_file(&path, "a: 1\n");
            let mut budget = u64::MAX;
            files.load(&project, &path, &mut budget).is_some()
        };
        for i in 0..MAX_CACHED {
            assert!(load(&mut files, i));
        }
        assert!(load(&mut files, 0), "used again");
        assert!(load(&mut files, MAX_CACHED));
        assert_eq!(files.cache.len(), MAX_CACHED);
        assert!(files.cache.contains_key("/project/f0.neon"));
        assert!(!files.cache.contains_key("/project/f1.neon"));
    }

    #[test]
    fn one_call_parses_a_bounded_amount_of_text() {
        let host = MockHost::install();
        let big = format!("parameters:\n\tx: {}\n", "a".repeat(1536 << 10));
        for name in ["a", "b", "c"] {
            host.add_file(&format!("/project/{name}.neon"), &big);
        }
        let mut files = Files::default();
        let current = Rc::new(Loaded::new(
            "/project/phpstan.neon".into(),
            "includes:\n\t- a.neon\n\t- b.neon\n\t- c.neon\n".into(),
        ));
        assert_eq!(files.related(current.clone()).len(), 3, "4 MiB: two of the three");
        assert_eq!(files.related(current.clone()).len(), 4, "the third with the next call");
        assert_eq!(files.related(current).len(), 4);
    }

    #[test]
    fn without_project_read_only_the_document_counts() {
        let host = MockHost::install().with_capabilities(&[]);
        host.add_file("/project/b.neon", "parameters:\n\tx: 1\n");
        let mut files = Files::default();
        let current = Rc::new(Loaded::new("/project/a.neon".into(), "includes:\n\t- b.neon\n".into()));
        assert_eq!(files.related(current).len(), 1);
    }
}
