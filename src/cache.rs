//! `prumo clean`: what the cache directory holds, and removing it. Every entry under the
//! root is one of: a document's artifacts (`<job>-<key>/`, with `<job>/run.json` naming
//! the document), the prune caches and outputs (`prune/`), or something else. Nothing is
//! removed without `--yes`; the listing is the plan.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Kind {
    /// A document's runs (`run`, `compare`, `unused` candidates).
    Document {
        /// The document the first run was for, from its `run.json`.
        main: PathBuf,
        /// Whether that document still exists where the run found it. Inside a container
        /// the path is the container's, so judge it from the same mount.
        exists: bool,
    },
    /// `prune` caches and outputs.
    Prune,
    /// Not written by this prumo, or a run that never got its `run.json`.
    Other,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub dir: PathBuf,
    #[serde(flatten)]
    pub kind: Kind,
    pub bytes: u64,
    /// Seconds since the entry was last written.
    pub age_secs: Option<u64>,
}

impl Entry {
    pub fn stale(&self) -> bool {
        matches!(self.kind, Kind::Document { exists: false, .. })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Listing {
    pub schema: u32,
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub bytes: u64,
}

/// Every entry under `root`, largest first. A missing root is an empty cache.
pub fn list(root: &Path) -> Listing {
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let kind = classify(&dir);
            let bytes = dir_size(&dir);
            let age_secs = newest(&dir).and_then(|t| SystemTime::now().duration_since(t).ok()).map(|d| d.as_secs());
            entries.push(Entry { dir, kind, bytes, age_secs });
        }
    }
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.dir.cmp(&b.dir)));
    let bytes = entries.iter().map(|e| e.bytes).sum();
    Listing { schema: 1, root: root.to_path_buf(), entries, bytes }
}

fn classify(dir: &Path) -> Kind {
    if dir.file_name().is_some_and(|n| n == "prune") {
        return Kind::Prune;
    }
    // `<job>-<key>/<job>/run.json`, or a `compare/<engine>/<job>/run.json` when only
    // `compare` ran.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            candidates.push(p.join("run.json"));
            if p.file_name().is_some_and(|n| n == "compare")
                && let Ok(engines) = std::fs::read_dir(&p)
            {
                for eng in engines.flatten() {
                    if let Ok(jobs) = std::fs::read_dir(eng.path()) {
                        candidates.extend(jobs.flatten().map(|j| j.path().join("run.json")));
                    }
                }
            }
        }
    }
    for info in candidates {
        if let Ok(text) = std::fs::read_to_string(&info)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(main) = v.get("main").and_then(|m| m.as_str())
        {
            let main = PathBuf::from(main);
            let exists = main.is_file();
            return Kind::Document { main, exists };
        }
    }
    Kind::Other
}

/// Bytes under a directory (file sizes, not blocks).
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                stack.push(e.path());
            } else {
                total += md.len();
            }
        }
    }
    total
}

fn newest(dir: &Path) -> Option<SystemTime> {
    let mut best: Option<SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = md.modified() {
                best = Some(best.map_or(m, |b| b.max(m)));
            }
        }
    }
    best
}

/// Remove the entries; returns what went. Stops at the first error.
pub fn remove(entries: &[Entry]) -> std::io::Result<(usize, u64)> {
    let mut n = 0;
    let mut bytes = 0;
    for e in entries {
        std::fs::remove_dir_all(&e.dir)?;
        n += 1;
        bytes += e.bytes;
    }
    Ok((n, bytes))
}

pub fn human_bytes(b: u64) -> String {
    const K: f64 = 1024.0;
    let b = b as f64;
    if b < K {
        format!("{b:.0} B")
    } else if b < K * K {
        format!("{:.0} kB", b / K)
    } else if b < K * K * K {
        format!("{:.1} MB", b / K / K)
    } else {
        format!("{:.2} GB", b / K / K / K)
    }
}

pub fn human_age(secs: Option<u64>) -> String {
    match secs {
        None => String::new(),
        Some(s) if s < 60 => "just now".into(),
        Some(s) if s < 3600 => format!("{} min ago", s / 60),
        Some(s) if s < 86_400 => format!("{} h ago", s / 3600),
        Some(s) => format!("{} d ago", s / 86_400),
    }
}

/// The listing as text, with `shown` turning paths into the user's own (container mounts).
pub fn text(l: &Listing, shown: impl Fn(&Path) -> String) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "cache: {}  ({} entr{}, {})",
        shown(&l.root),
        l.entries.len(),
        if l.entries.len() == 1 { "y" } else { "ies" },
        human_bytes(l.bytes)
    );
    let w = l.entries.iter().map(|e| e.dir.file_name().map_or(0, |n| n.len())).max().unwrap_or(8);
    for e in &l.entries {
        let name = e.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let what = match &e.kind {
            Kind::Document { main, exists: true } => shown(main),
            Kind::Document { main, exists: false } => format!("{}  (document missing)", shown(main)),
            Kind::Prune => "prune caches and outputs".into(),
            Kind::Other => "not a prumo run".into(),
        };
        let _ = writeln!(s, "  {name:<w$}  {:>9}  {what}  {}", human_bytes(e.bytes), human_age(e.age_secs));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_json(dir: &Path, main: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("run.json"), format!("{{\"main\": {:?}, \"job\": \"x\"}}", main.display())).unwrap();
        std::fs::write(dir.join("x.log"), "0123456789").unwrap();
    }

    #[test]
    fn lists_documents_prune_and_strangers_and_removes_what_is_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");
        let doc = tmp.path().join("thesis.tex");
        std::fs::write(&doc, "x").unwrap();
        run_json(&root.join("thesis-0123456789/thesis"), &doc);
        run_json(&root.join("gone-abcdefabcd/gone"), &tmp.path().join("gone.tex"));
        run_json(&root.join("cmp-1111111111/compare/xelatex/cmp"), &doc);
        std::fs::create_dir_all(root.join("prune/corpus")).unwrap();
        std::fs::write(root.join("prune/keep.txt"), "lm\n").unwrap();
        std::fs::create_dir_all(root.join("stranger")).unwrap();
        std::fs::write(root.join("a-file"), "ignored").unwrap();

        let l = list(&root);
        let by = |n: &str| l.entries.iter().find(|e| e.dir.file_name().unwrap() == n).unwrap();
        assert_eq!(l.entries.len(), 5);
        assert_eq!(by("thesis-0123456789").kind, Kind::Document { main: doc.clone(), exists: true });
        assert!(by("gone-abcdefabcd").stale());
        assert!(matches!(by("cmp-1111111111").kind, Kind::Document { exists: true, .. }), "compare-only entry");
        assert_eq!(by("prune").kind, Kind::Prune);
        assert_eq!(by("stranger").kind, Kind::Other);
        assert_eq!(
            by("thesis-0123456789").bytes,
            10 + by("thesis-0123456789").dir.join("thesis/run.json").metadata().unwrap().len()
        );
        assert_eq!(l.bytes, l.entries.iter().map(|e| e.bytes).sum::<u64>());
        assert!(l.entries.windows(2).all(|w| w[0].bytes >= w[1].bytes), "largest first");

        let t = text(&l, |p| p.display().to_string());
        assert!(
            t.contains("(document missing)") && t.contains("prune caches and outputs") && t.contains("5 entries"),
            "{t}"
        );

        let stale: Vec<Entry> = l.entries.iter().filter(|e| e.stale()).cloned().collect();
        let (n, _) = remove(&stale).unwrap();
        assert_eq!(n, 1);
        assert!(!root.join("gone-abcdefabcd").exists() && root.join("thesis-0123456789").exists());
        assert!(list(&tmp.path().join("nowhere")).entries.is_empty(), "a missing root is an empty cache");
    }

    #[test]
    fn humanized() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 kB");
        assert_eq!(human_bytes(3 * 1024 * 1024 + 1024 * 512), "3.5 MB");
        assert_eq!(human_age(Some(30)), "just now");
        assert_eq!(human_age(Some(7200)), "2 h ago");
        assert_eq!(human_age(Some(3 * 86_400)), "3 d ago");
        assert_eq!(human_age(None), "");
    }
}
