//! `prumo packages`: the TeX Live packages one build needs.
//!
//! `used files → tlpdb file index → packages`, plus the infrastructure the `.fls` cannot
//! prove (formats are prebuilt, so `latex.ltx` and the hyphenation patterns never appear as
//! INPUT), closed over `depend`. Every entry says why it is there and, for used files, which
//! LaTeX unit loaded the file. See `docs/05-packages-and-pruning.md`.

use crate::fls::{InputFile, InputKind, Tree};
use crate::model::{BuildModel, Frame, FrameKind};
use crate::records::Engine;
use crate::tlpdb::{Category, Tlpdb};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Why a package is in the list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Reason {
    /// A file of this package was read; `via` names the LaTeX unit that loaded it.
    Used { file: PathBuf, via: String },
    /// TeX Live's own machinery: `texlive.infra`, `kpathsea`, `texlive-scripts`, `hyphen-base`.
    Infra,
    /// The engine binary the `start` record names.
    Engine,
    /// Provides the format that ran (`execute AddFormat name=<fmt>`), or is one of that
    /// line's `fmttriggers`, which the format was built from.
    Format { format: String },
    /// Pulled in by another package's `depend`.
    Depend { of: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackageUse {
    pub name: String,
    pub category: Option<Category>,
    pub reason: Reason,
    /// `runfiles`+`binfiles` size in bytes, from the tlpdb.
    pub size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct PackagesReport {
    pub schema: u32,
    pub job: String,
    pub format: String,
    /// Sorted by name.
    pub packages: Vec<PackageUse>,
    /// `texmf-dist` files the database does not list (local edits, a different TeX Live).
    pub unresolved: Vec<PathBuf>,
    /// Files from `TEXMFLOCAL`/`TEXMFHOME`: not tlmgr-managed; the user is on their own.
    pub local: Vec<PathBuf>,
    pub total_size: u64,
}

/// The infrastructure packages that are always needed; see `docs/05-packages-and-pruning.md`.
pub const INFRA: [&str; 4] = ["texlive.infra", "kpathsea", "texlive-scripts", "hyphen-base"];

/// The format a `latex` command runs.
pub fn format_of(cmd: &str) -> &str {
    cmd.rsplit('/').next().unwrap_or(cmd)
}

/// `name → loader` for every file and unit in the tree: `siunitx.sty → obra.cls`,
/// `t1pcr.fd → main.tex`, `translations.sty → siunitx`.
pub fn loaders(m: &BuildModel) -> HashMap<String, String> {
    fn label(fr: &Frame) -> String {
        match fr.kind {
            FrameKind::Job => "the command line".into(),
            FrameKind::Cls => format!("{}.cls", fr.name),
            _ => fr.name.clone(),
        }
    }
    fn go(fr: &Frame, out: &mut HashMap<String, String>) {
        for c in &fr.children {
            let by = label(fr);
            let key = match c.kind {
                FrameKind::Pkg => format!("{}.sty", c.name),
                FrameKind::Cls => format!("{}.cls", c.name),
                _ => c.name.rsplit('/').next().unwrap_or(&c.name).to_string(),
            };
            out.entry(key).or_insert(by);
            go(c, out);
        }
    }
    let mut out = HashMap::new();
    go(&m.root, &mut out);
    out
}

/// The whole computation. `format` is `pdflatex`/`xelatex`/`lualatex`.
pub fn resolve(job: &str, model: &BuildModel, files: &[InputFile], db: &Tlpdb, format: &str) -> PackagesReport {
    let via = loaders(model);
    let mut report =
        PackagesReport { schema: 1, job: job.to_string(), format: format.to_string(), ..Default::default() };
    // name → reason, first reason wins; used files are the strongest evidence.
    let mut reasons: BTreeMap<String, Reason> = BTreeMap::new();
    for f in files {
        match f.tree {
            Tree::TexmfLocal | Tree::TexmfHome => {
                report.local.push(f.path.clone());
                continue;
            }
            Tree::TexmfDist => {}
            _ => continue,
        }
        match db.package_for(&f.path) {
            Some(pkg) => {
                let base = f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let who = via.get(&base).cloned().unwrap_or_else(|| match f.kind {
                    k if k.is_font() => "(font load)".into(),
                    InputKind::Map | InputKind::Enc => "(font map)".into(),
                    InputKind::Cnf | InputKind::Fmt => "(engine startup)".into(),
                    _ => "(outside hook coverage)".into(),
                });
                reasons.entry(pkg.to_string()).or_insert(Reason::Used { file: f.path.clone(), via: who });
            }
            None => report.unresolved.push(f.path.clone()),
        }
    }
    for p in INFRA {
        reasons.entry(p.into()).or_insert(Reason::Infra);
    }
    if let Some(p) = model.engine.package() {
        reasons.entry(p.into()).or_insert(Reason::Engine);
        if model.engine == Engine::LuaTex && model.engine_name == "luahbtex" {
            reasons.entry("luahbtex".into()).or_insert(Reason::Engine);
        }
    }
    for p in db.format_providers(format) {
        reasons.entry(p.to_string()).or_insert(Reason::Format { format: format.into() });
        for t in fmttriggers(db, p, format) {
            reasons.entry(t).or_insert(Reason::Format { format: format.into() });
        }
    }
    // depend closure, remembering who pulled each package in.
    let roots: Vec<String> = reasons.keys().cloned().collect();
    let mut todo: Vec<(String, String)> = roots.iter().map(|r| (r.clone(), r.clone())).collect();
    while let Some((name, of)) = todo.pop() {
        if !reasons.contains_key(&name) {
            reasons.insert(name.clone(), Reason::Depend { of: of.clone() });
        } else if name != of {
            continue;
        }
        if let Some(p) = db.packages.get(&name) {
            for d in &p.depends {
                if d.contains(':') || d.contains('/') {
                    continue;
                }
                if let Some(base) = d.strip_suffix(".ARCH") {
                    let prefix = format!("{base}.");
                    for n in db.packages.keys().filter(|n| n.starts_with(&prefix)) {
                        if !reasons.contains_key(n) {
                            todo.push((n.clone(), name.clone()));
                        }
                    }
                } else if !reasons.contains_key(d) {
                    todo.push((d.clone(), name.clone()));
                }
            }
        }
    }
    for (name, reason) in reasons {
        let (category, size) = db.packages.get(&name).map(|p| (p.category, p.run_size)).unwrap_or((None, 0));
        report.total_size += size;
        report.packages.push(PackageUse { name, category, reason, size });
    }
    report.unresolved.sort();
    report.unresolved.dedup();
    report.local.sort();
    report
}

/// The `fmttriggers=a,b,c` list of the `AddFormat name=<format>` line of `provider`.
fn fmttriggers(db: &Tlpdb, provider: &str, format: &str) -> Vec<String> {
    let needle = format!("name={format} ");
    db.packages
        .get(provider)
        .into_iter()
        .flat_map(|p| p.executes.iter())
        .filter(|e| e.verb == "AddFormat" && format!("{} ", e.args).contains(&needle))
        .flat_map(|e| {
            e.args
                .split_whitespace()
                .find_map(|a| a.strip_prefix("fmttriggers="))
                .unwrap_or("")
                .split(',')
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Text rendering: one line per package, used ones first with their file and loader.
pub fn text(r: &PackagesReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let used: Vec<&PackageUse> = r.packages.iter().filter(|p| matches!(p.reason, Reason::Used { .. })).collect();
    let rest: Vec<&PackageUse> = r.packages.iter().filter(|p| !matches!(p.reason, Reason::Used { .. })).collect();
    let _ = writeln!(
        s,
        "== prumo packages: {}  ({} packages, {} MB; {} from files read, {} infrastructure and dependencies; format {})",
        r.job,
        r.packages.len(),
        r.total_size / 1_000_000,
        used.len(),
        rest.len(),
        r.format
    );
    let w = r.packages.iter().map(|p| p.name.len()).max().unwrap_or(10);
    let _ = writeln!(s, "\n-- used --");
    for p in &used {
        if let Reason::Used { file, via } = &p.reason {
            let _ = writeln!(s, "  {:<w$}  {:<44}  via {via}", p.name, short(file));
        }
    }
    let _ = writeln!(s, "\n-- needed for the engine, format and dependencies --");
    let mut line = String::from("  ");
    for p in &rest {
        let tag = match &p.reason {
            Reason::Infra => "infra".to_string(),
            Reason::Engine => "engine".to_string(),
            Reason::Format { format } => format!("format {format}"),
            Reason::Depend { of } => format!("← {of}"),
            Reason::Used { .. } => unreachable!(),
        };
        let item = format!("{} ({tag})  ", p.name);
        if line.len() + item.len() > 100 {
            let _ = writeln!(s, "{}", line.trim_end());
            line = String::from("  ");
        }
        line.push_str(&item);
    }
    let _ = writeln!(s, "{}", line.trim_end());
    if !r.unresolved.is_empty() {
        let _ = writeln!(s, "\n-- texmf-dist files not in the database ({}) --", r.unresolved.len());
        for f in &r.unresolved {
            let _ = writeln!(s, "  {}", short(f));
        }
    }
    if !r.local.is_empty() {
        let _ = writeln!(s, "\n-- not tlmgr-managed (TEXMFLOCAL/TEXMFHOME) --");
        for f in &r.local {
            let _ = writeln!(s, "  {}", f.display());
        }
    }
    s
}

/// `…/texmf-dist/tex/latex/siunitx/siunitx.sty` → `tex/latex/siunitx/siunitx.sty`.
fn short(p: &Path) -> String {
    Tlpdb::key_for(p)
        .map(|k| k.trim_start_matches("texmf-dist/").to_string())
        .unwrap_or_else(|| p.display().to_string())
}

/// With no tlpdb (Debian): the texmf-dist files grouped by their directory under `texmf-dist`.
pub fn file_level_text(job: &str, files: &[InputFile]) -> String {
    use std::fmt::Write as _;
    let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in files.iter().filter(|f| f.tree == Tree::TexmfDist) {
        let key = short(&f.path);
        let (dir, name) = key.rsplit_once('/').unwrap_or(("", &key));
        by_dir.entry(dir.to_string()).or_default().push(name.to_string());
    }
    let mut s = format!(
        "== prumo packages: {job}  (no texlive.tlpdb found: file-level only; {} texmf-dist files)\n",
        by_dir.values().map(Vec::len).sum::<usize>()
    );
    for (dir, names) in by_dir {
        let _ = writeln!(s, "  {dir}/  {}", names.join(" "));
    }
    s
}
