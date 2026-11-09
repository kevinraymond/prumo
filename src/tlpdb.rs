//! Parser for `texlive.tlpdb`, the TeX Live package database, and the two things prumo
//! asks of it: which package owns a file, and the `depend` closure of a package set.
//!
//! Format (verified against TeX Live 2026, see `fixtures/tlpdb/excerpt.tlpdb`): stanzas
//! separated by blank lines, `key value` lines, and file lists introduced by
//! `runfiles size=N` / `docfiles size=N` / `srcfiles size=N` / `binfiles arch=A size=N`
//! whose entries are the following lines that start with one space. `size` is in blocks
//! of 4096 bytes (`TeXLive::TLConfig::BlockSize`).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::{Component, Path};

/// `TeXLive::TLConfig::BlockSize`: the unit of every `size=` in the database.
pub(crate) const BLOCK_SIZE: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Category {
    Package,
    Collection,
    Scheme,
    TLCore,
    ConTeXt,
    /// A category this parser does not know; kept so nothing is silently dropped.
    Other,
}

impl Category {
    fn parse(s: &str) -> Category {
        match s {
            "Package" => Category::Package,
            "Collection" => Category::Collection,
            "Scheme" => Category::Scheme,
            "TLCore" => Category::TLCore,
            "ConTeXt" => Category::ConTeXt,
            _ => Category::Other,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Category::Package => "Package",
            Category::Collection => "Collection",
            Category::Scheme => "Scheme",
            Category::TLCore => "TLCore",
            Category::ConTeXt => "ConTeXt",
            Category::Other => "Other",
        })
    }
}

/// One `execute` line, split into its verb and argument text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execute {
    /// `addMap`, `addMixedMap`, `addKanjiMap`, `AddFormat`, `AddHyphen`.
    pub verb: String,
    pub args: String,
}

/// One package stanza.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tlpkg {
    pub name: String,
    pub category: Option<Category>,
    pub revision: Option<u64>,
    pub shortdesc: Option<String>,
    pub depends: Vec<String>,
    pub executes: Vec<Execute>,
    /// `texmf-dist/...` paths; `RELOC/` (remote databases) is rewritten to `texmf-dist/`.
    pub runfiles: Vec<String>,
    /// `bin/<arch>/...` paths from `<pkg>.<arch>` stanzas.
    pub binfiles: Vec<String>,
    pub docfiles: Vec<String>,
    pub srcfiles: Vec<String>,
    /// Sum of the `size=` fields of runfiles and binfiles, in bytes.
    pub run_size: u64,
    pub relocated: bool,
}

/// The parsed database with its file index.
#[derive(Debug, Default)]
pub struct Tlpdb {
    pub packages: HashMap<String, Tlpkg>,
    /// `texmf-dist/tex/latex/hyperref/hyperref.sty` → `hyperref`; also `bin/x86_64-linux/pdftex` → `pdftex.x86_64-linux`.
    pub file_index: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TlpdbError {
    #[error("{file}:{line}: {msg}")]
    Line { file: String, line: usize, msg: String },
    #[error("{file}: cannot read: {source}")]
    Io {
        file: String,
        #[source]
        source: std::io::Error,
    },
}

/// Parse a database from disk.
pub fn parse_file(path: &Path) -> Result<Tlpdb, TlpdbError> {
    let file = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|source| TlpdbError::Io { file: file.clone(), source })?;
    parse_str(&String::from_utf8_lossy(&bytes), &file)
}

/// Parse database text. Lines starting with `#` are skipped (the fixture excerpt has
/// comments; real databases do not).
pub(crate) fn parse_str(text: &str, file: &str) -> Result<Tlpdb, TlpdbError> {
    let err = |line: usize, msg: String| TlpdbError::Line { file: file.to_string(), line, msg };
    let mut db = Tlpdb::default();
    let mut cur: Option<Tlpkg> = None;
    // Which file list the indented lines belong to, with the stanza line that opened it.
    let mut list: Option<FileList> = None;

    let finish = |cur: &mut Option<Tlpkg>, db: &mut Tlpdb| {
        if let Some(p) = cur.take() {
            for f in p.runfiles.iter().chain(&p.binfiles) {
                db.file_index.entry(f.clone()).or_insert_with(|| p.name.clone());
            }
            db.packages.insert(p.name.clone(), p);
        }
    };

    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let raw = raw.trim_end_matches('\r');
        if raw.starts_with('#') {
            continue;
        }
        if raw.trim().is_empty() {
            finish(&mut cur, &mut db);
            list = None;
            continue;
        }
        if let Some(entry) = raw.strip_prefix(' ') {
            let Some(kind) = list else {
                return Err(err(line, "indented file entry outside a file list".into()));
            };
            let Some(p) = cur.as_mut() else {
                return Err(err(line, "file entry before any `name`".into()));
            };
            // Only the path matters; `details="..."`, `language=..` decorations are dropped.
            let path = entry.split(' ').next().unwrap_or_default();
            let path =
                path.strip_prefix("RELOC/").map(|p| format!("texmf-dist/{p}")).unwrap_or_else(|| path.to_string());
            match kind {
                FileList::Run => p.runfiles.push(path),
                FileList::Bin => p.binfiles.push(path),
                FileList::Doc => p.docfiles.push(path),
                FileList::Src => p.srcfiles.push(path),
            }
            continue;
        }
        list = None;
        let (key, value) = raw.split_once(' ').unwrap_or((raw, ""));
        let value = value.trim();
        if key == "name" {
            finish(&mut cur, &mut db);
            if value.is_empty() {
                return Err(err(line, "`name` without a value".into()));
            }
            cur = Some(Tlpkg { name: value.to_string(), ..Default::default() });
            continue;
        }
        let Some(p) = cur.as_mut() else {
            return Err(err(line, format!("`{key}` before any `name`")));
        };
        match key {
            "category" => p.category = Some(Category::parse(value)),
            "revision" => {
                p.revision =
                    Some(value.parse().map_err(|_| err(line, format!("revision is not an integer: {value:?}")))?);
            }
            "shortdesc" => p.shortdesc = Some(value.to_string()),
            "depend" => p.depends.push(value.to_string()),
            "execute" => {
                let (verb, args) = value.split_once(' ').unwrap_or((value, ""));
                p.executes.push(Execute { verb: verb.to_string(), args: args.trim().to_string() });
            }
            "relocated" => p.relocated = value == "1",
            "runfiles" | "binfiles" | "docfiles" | "srcfiles" => {
                let size = size_field(value).ok_or_else(|| err(line, format!("`{key}` without size=")))?;
                list = Some(match key {
                    "runfiles" => FileList::Run,
                    "binfiles" => FileList::Bin,
                    "docfiles" => FileList::Doc,
                    _ => FileList::Src,
                });
                if matches!(list, Some(FileList::Run | FileList::Bin)) {
                    p.run_size += size * BLOCK_SIZE;
                }
            }
            // longdesc, catalogue-*, containersize, containerchecksum, postaction, ...
            _ => {}
        }
    }
    finish(&mut cur, &mut db);
    Ok(db)
}

#[derive(Debug, Clone, Copy)]
enum FileList {
    Run,
    Bin,
    Doc,
    Src,
}

fn size_field(attrs: &str) -> Option<u64> {
    attrs.split_whitespace().find_map(|a| a.strip_prefix("size=")).and_then(|s| s.parse().ok())
}

impl Tlpdb {
    /// Turn an absolute path into the database key: everything from the `texmf-dist`
    /// component on (or from `bin/` for binaries under `TEXMFROOT`).
    pub fn key_for(path: &Path) -> Option<String> {
        let comps: Vec<&str> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();
        let start = comps.iter().position(|c| *c == "texmf-dist")?;
        Some(comps[start..].join("/"))
    }

    /// The package that ships `path`, if the path is under a `texmf-dist` tree and listed.
    pub fn package_for(&self, path: &Path) -> Option<&str> {
        let key = Self::key_for(path)?;
        self.file_index.get(&key).map(String::as_str)
    }

    /// Transitive `depend` closure of `roots`, sorted. Names the database does not know
    /// are kept in the result (a missing package is information, not an error), and
    /// `depend` values that are not package names (`platform:…`, `opt_…`, `release/…`,
    /// the `00texlive.*` bookkeeping stanzas use them) are skipped.
    pub fn closure<'a, I: IntoIterator<Item = &'a str>>(&self, roots: I) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        self.walk_depends(roots, |name, _| {
            out.insert(name.to_string());
        });
        out
    }

    /// The walk behind `closure`: every package `roots` pull in, reported once to
    /// `reached(name, by)` with the package whose `depend` line got there first (a root
    /// reports itself). The same skipping rules as `closure`.
    pub fn walk_depends<'a, I: IntoIterator<Item = &'a str>>(&self, roots: I, mut reached: impl FnMut(&str, &str)) {
        let mut seen = BTreeSet::new();
        let mut todo: Vec<(String, String)> = roots.into_iter().map(|r| (r.to_string(), r.to_string())).collect();
        while let Some((name, by)) = todo.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            reached(&name, &by);
            if let Some(p) = self.packages.get(&name) {
                for d in &p.depends {
                    if d.contains(':') || d.contains('/') {
                        continue;
                    }
                    // `foo.ARCH` means every installed `foo.<arch>` binary stanza.
                    if let Some(base) = d.strip_suffix(".ARCH") {
                        let prefix = format!("{base}.");
                        for n in self.packages.keys().filter(|n| n.starts_with(&prefix) && !n.ends_with(".ARCH")) {
                            if !seen.contains(n) {
                                todo.push((n.clone(), name.clone()));
                            }
                        }
                        continue;
                    }
                    if !seen.contains(d) {
                        todo.push((d.clone(), name.clone()));
                    }
                }
            }
        }
    }

    /// Packages that provide a format via `execute AddFormat name=<fmt> …`.
    pub fn format_providers(&self, format: &str) -> Vec<&str> {
        let needle = format!("name={format} ");
        let mut v: Vec<&str> = self
            .packages
            .values()
            .filter(|p| p.executes.iter().any(|e| e.verb == "AddFormat" && format!("{} ", e.args).contains(&needle)))
            .map(|p| p.name.as_str())
            .collect();
        v.sort_unstable();
        v
    }

    /// Packages that register a font map (`addMap`, `addMixedMap`, `addKanjiMap`).
    pub fn is_font_package(&self, name: &str) -> bool {
        self.packages.get(name).is_some_and(|p| {
            p.executes.iter().any(|e| matches!(e.verb.as_str(), "addMap" | "addMixedMap" | "addKanjiMap"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excerpt() -> Tlpdb {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tlpdb/excerpt.tlpdb");
        parse_file(&path).unwrap()
    }

    #[test]
    fn parses_the_real_excerpt() {
        let db = excerpt();
        assert_eq!(db.packages.len(), 129);
        let h = &db.packages["hyperref"];
        assert_eq!(h.category, Some(Category::Package));
        assert!(h.depends.iter().any(|d| d == "url"));
        assert!(h.runfiles.contains(&"texmf-dist/tex/latex/hyperref/hyperref.sty".to_string()));
        assert!(h.docfiles.len() <= 3, "the image has no docs installed; the excerpt caps lists at 3 anyway");
        assert!(h.run_size > 0 && h.run_size.is_multiple_of(BLOCK_SIZE));
        assert_eq!(db.packages["scheme-infraonly"].category, Some(Category::Scheme));
        assert!(db.packages["scheme-infraonly"].relocated);
        let bin = &db.packages["pdftex.x86_64-linux"];
        assert_eq!(bin.category, Some(Category::TLCore));
        assert!(bin.binfiles.contains(&"bin/x86_64-linux/pdftex".to_string()));
        assert!(db.is_font_package("cm-super"));
        assert!(db.is_font_package("lm"));
        assert!(!db.is_font_package("hyperref"));
        assert_eq!(db.format_providers("pdflatex"), vec!["latex-bin"]);
        assert_eq!(db.format_providers("xelatex"), vec!["xetex"]);
        assert_eq!(db.format_providers("lualatex"), vec!["latex-bin"]);
    }

    #[test]
    fn file_index_resolves_absolute_paths() {
        let db = excerpt();
        let p = Path::new("/usr/local/texlive/2026/texmf-dist/tex/latex/hyperref/hyperref.sty");
        assert_eq!(db.package_for(p), Some("hyperref"));
        let p = Path::new("/usr/share/texlive/texmf-dist/tex/latex/psnfss/t1pcr.fd");
        assert_eq!(db.package_for(p), Some("psnfss"), "Debian layout works too");
        let p = Path::new("/usr/local/texlive/2026/texmf-dist/fonts/tfm/adobe/courier/pcrr8t.tfm");
        assert_eq!(db.package_for(p), Some("courier"));
        let p = Path::new("/usr/local/texlive/2026/texmf-dist/fonts/type1/public/amsfonts/cm/cmr10.pfb");
        assert_eq!(db.package_for(p), Some("amsfonts"));
        assert_eq!(db.package_for(Path::new("/var/lib/texmf/web2c/pdftex/pdflatex.fmt")), None);
        assert_eq!(db.package_for(Path::new("/usr/local/texlive/2026/texmf-dist/tex/latex/nope/x.sty")), None);
    }

    #[test]
    fn closure_follows_depends_and_skips_bookkeeping() {
        let db = excerpt();
        let c = db.closure(["hyperref"]);
        assert!(c.contains("hyperref"));
        assert!(c.contains("url"), "direct depend");
        assert!(c.contains("kvoptions"), "transitive: hyperref → … → kvoptions: {c:?}");
        assert_eq!(db.closure(["no-such-package"]).len(), 1, "unknown names stay, no panic");
        let c = db.closure(["00texlive.installation"]);
        assert_eq!(c.len(), 1, "platform:/opt_/release/ depends are not packages: {c:?}");
        let c = db.closure(["collection-basic"]);
        assert!(c.contains("kpathsea"), "{c:?}");
        let c = db.closure(["xetex"]);
        assert!(c.contains("dvipdfmx.x86_64-linux"), "`.ARCH` depends resolve to the installed arch stanza: {c:?}");
        assert!(!c.iter().any(|n| n.ends_with(".ARCH")));
    }

    #[test]
    fn hand_written_stanzas_and_errors() {
        let db = parse_str(
            "name a\ncategory Package\nrevision 1\ndepend b\nrunfiles size=2\n RELOC/tex/a.sty\ndocfiles size=1\n RELOC/doc/a.pdf details=\"Manual\"\n\nname b\ncategory Package\n",
            "t",
        )
        .unwrap();
        assert_eq!(db.file_index["texmf-dist/tex/a.sty"], "a");
        assert_eq!(db.packages["a"].run_size, 2 * BLOCK_SIZE, "doc size does not count");
        assert_eq!(db.packages["a"].docfiles, vec!["texmf-dist/doc/a.pdf"], "attributes after the path are dropped");
        assert_eq!(db.closure(["a"]).into_iter().collect::<Vec<_>>(), vec!["a", "b"]);
        let e = parse_str("category Package\n", "t").unwrap_err();
        assert_eq!(e.to_string(), "t:1: `category` before any `name`");
        let e = parse_str("name a\n x\n", "t").unwrap_err();
        assert_eq!(e.to_string(), "t:2: indented file entry outside a file list");
        let e = parse_str("name a\nrevision x\n", "t").unwrap_err();
        assert!(e.to_string().contains("revision is not an integer"));
    }
}
