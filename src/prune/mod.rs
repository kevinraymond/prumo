//! `prumo prune`: from "files these documents read" to "packages you can remove".
//!
//! Rule 2 of the charter: this prints a plan. It runs `tlmgr remove` only with `--apply`,
//! and `--apply` first recomputes the plan, shows it, and requires `--yes`.
//!
//! `plan()` is pure (database + per-document package reports in, plan out) so the whole
//! computation is tested on `fixtures/tlpdb/excerpt.tlpdb` without TeX; `run_corpus()` does
//! the builds; `apply()` talks to `tlmgr`.

pub mod keep;

use crate::fls::{self, Trees};
use crate::fonts::{self, PdfFont};
use crate::packages::{self, PackagesReport, Reason};
use crate::run::{self, EngineCmd, RunOptions};
use crate::tlpdb::{Category, Tlpdb};
use keep::KeepReason;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Hyphenation {
    All,
    Used,
}

#[derive(Debug, Clone)]
pub struct PruneOptions {
    /// Files, directories or glob patterns.
    pub paths: Vec<String>,
    /// Engines each document runs on; a `% !TEX program =` magic comment overrides.
    pub engines: Vec<EngineCmd>,
    pub latexmk: bool,
    pub keep: Vec<String>,
    pub keep_hyphenation: Hyphenation,
    /// Where caches and outputs go (the CLI defaults to `<cache root>/prune`).
    pub out: PathBuf,
    pub env: Vec<(String, String)>,
}

/// One instrumented build of the corpus and what it proved.
#[derive(Debug, Clone)]
pub struct DocRun {
    pub main: PathBuf,
    pub engine: EngineCmd,
    pub latexmk: bool,
    pub packages: PackagesReport,
    pub pdf_fonts: Vec<PdfFont>,
    /// `<lang>.ldf` basenames read (for `--keep-hyphenation used`).
    pub languages: Vec<String>,
    /// Build tools the `.fls` proves (`build_tools`): their tlpdb package names.
    pub tools: Vec<String>,
}

/// Build tools a run needs, by what the `.fls` shows: `<job>.bcf` written → `biber`;
/// `<job>.bbl` read with no `.bcf` → `bibtex`; `<job>.ind` read → `makeindex`. TeX never
/// reads these programs' files itself, so without this a prune would remove them.
pub fn build_tools(lines: &[fls::FlsLine], job: &str) -> Vec<String> {
    let named = |p: &PathBuf, ext: &str| p.file_name().is_some_and(|n| n.to_string_lossy() == format!("{job}.{ext}"));
    let outputs = fls::outputs(lines);
    let inputs = fls::inputs(lines);
    let mut tools = Vec::new();
    let bcf = outputs.iter().any(|p| named(p, "bcf"));
    if bcf {
        tools.push("biber".to_string());
    } else if inputs.iter().any(|p| named(p, "bbl")) {
        tools.push("bibtex".to_string());
    }
    if inputs.iter().any(|p| named(p, "ind")) {
        tools.push("makeindex".to_string());
    }
    tools
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub schema: u32,
    pub documents: Vec<DocSummary>,
    pub installed: usize,
    pub installed_size: u64,
    /// Package → why it stays. `.ARCH` stanzas included.
    pub needed: BTreeMap<String, KeepReason>,
    pub needed_size: u64,
    /// Schemes to remove first, then collections (meta-packages: no files of their own).
    /// tlmgr refuses a package while an installed scheme or collection lists it, so each
    /// group goes in its own `tlmgr remove` call.
    pub schemes: Vec<String>,
    pub collections: Vec<String>,
    /// `schemes` followed by `collections`.
    pub meta: Vec<String>,
    /// Packages to remove, dependents before their dependencies.
    pub removable: Vec<String>,
    /// Everything to remove (schemes, collections, packages) in one dependents-first order:
    /// what `commands()` and `apply()` use.
    pub order: Vec<String>,
    pub removable_size: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DocSummary {
    pub main: PathBuf,
    pub engine: EngineCmd,
    pub packages_used: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum PruneError {
    #[error(transparent)]
    Run(#[from] run::RunError),
    #[error("no documents found under {} (a document is a .tex file containing \\documentclass)", .0.join(", "))]
    NoDocuments(Vec<String>),
    #[error("tlmgr remove exited with {code:?}; see {log}")]
    Tlmgr { code: Option<i32>, log: PathBuf },
    #[error("bad pattern {pattern}: {source}")]
    Pattern {
        pattern: String,
        #[source]
        source: glob::PatternError,
    },
    #[error("cannot {what} {path}: {source}")]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("--apply needs --yes: the plan above would run `tlmgr remove` on this machine")]
    NeedsYes,
    #[error("tlmgr is not on PATH; nothing was changed")]
    NoTlmgr,
    #[error("{0}")]
    Model(#[from] crate::model::ModelError),
}

/// `.tex` files containing `\documentclass` under the given files, directories or globs.
pub fn discover(paths: &[String]) -> Result<Vec<PathBuf>, PruneError> {
    let mut found = BTreeSet::new();
    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            walk(path, &mut found)?;
        } else if path.is_file() {
            found.insert(path.to_path_buf());
        } else {
            for entry in glob::glob(p).map_err(|source| PruneError::Pattern { pattern: p.clone(), source })?.flatten() {
                if entry.is_dir() {
                    walk(&entry, &mut found)?;
                } else {
                    found.insert(entry);
                }
            }
        }
    }
    let docs: Vec<PathBuf> =
        found.into_iter().filter(|p| p.extension().is_some_and(|e| e == "tex") && is_document(p)).collect();
    if docs.is_empty() {
        return Err(PruneError::NoDocuments(paths.to_vec()));
    }
    Ok(docs)
}

fn walk(dir: &Path, out: &mut BTreeSet<PathBuf>) -> Result<(), PruneError> {
    for e in std::fs::read_dir(dir)
        .map_err(|source| PruneError::Io { what: "read", path: dir.to_path_buf(), source })?
        .flatten()
    {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !name.starts_with('.') && name != "target" {
                walk(&p, out)?;
            }
        } else if name.ends_with(".tex") {
            out.insert(p);
        }
    }
    Ok(())
}

/// A `.tex` file with an uncommented `\documentclass`.
pub fn is_document(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|t| t.lines().any(|l| l.split('%').next().unwrap_or("").contains("\\documentclass")))
        .unwrap_or(false)
}

/// `% !TEX program = xelatex` (TeXstudio, TeXShop, VS Code conventions) in the first lines.
pub fn magic_engine(path: &Path) -> Option<EngineCmd> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines().take(20) {
        let l = line.trim().trim_start_matches('%').trim();
        let Some(rest) = l.strip_prefix("!TEX").or_else(|| l.strip_prefix("!TeX")) else { continue };
        let rest = rest.trim();
        let Some((key, value)) = rest.split_once('=') else { continue };
        if key.trim().eq_ignore_ascii_case("program") || key.trim().eq_ignore_ascii_case("TS-program") {
            return match value.trim().to_ascii_lowercase().as_str() {
                "pdflatex" => Some(EngineCmd::Pdflatex),
                "xelatex" => Some(EngineCmd::Xelatex),
                "lualatex" => Some(EngineCmd::Lualatex),
                _ => None,
            };
        }
    }
    None
}

/// Build every document on its engines (cached) and collect what each run proved.
pub fn run_corpus(opts: &PruneOptions, db: &Tlpdb, mut progress: impl FnMut(&str)) -> Result<Vec<DocRun>, PruneError> {
    let docs = discover(&opts.paths)?;
    let mut runs = Vec::new();
    for main in docs {
        let engines = match magic_engine(&main) {
            Some(e) => vec![e],
            None => opts.engines.clone(),
        };
        for engine in engines {
            let out_dir = opts.out.join("corpus").join(engine.command()).join(stable_key(&main));
            progress(&format!("{} on {}", main.display(), engine.command()));
            let ro = RunOptions {
                main: main.clone(),
                engine,
                latexmk: opts.latexmk,
                build: None,
                out: Some(out_dir),
                env: opts.env.clone(),
                pretex: None,
                edits: vec![],
                timeout: None,
            };
            let cached = run::cached(&main, ro.out.as_deref())?.filter(|c| !run::is_stale(c, &ro));
            let out = match cached {
                Some(c) => c,
                None => run::run(&ro)?,
            };
            let model = run::model_of(&out)?;
            let lines = fls::parse_file(&out.fls).map_err(run::RunError::Fls)?;
            let trees =
                Trees::from_kpsewhich(&std::fs::read_to_string(out.dir.join("kpsewhich.txt")).unwrap_or_default());
            let files = fls::input_files(&lines, &trees);
            let packages = packages::resolve(&out.job, &model, &files, db, engine.command());
            let pdf_fonts = std::fs::read_to_string(out.dir.join("pdffonts.txt"))
                .map(|t| fonts::parse_pdffonts(&t))
                .unwrap_or_default();
            let languages = files
                .iter()
                .filter(|f| f.kind == fls::InputKind::Ldf)
                .filter_map(|f| f.path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect();
            let tools = build_tools(&lines, &out.job);
            runs.push(DocRun {
                main: main.clone(),
                engine,
                latexmk: opts.latexmk,
                packages,
                pdf_fonts,
                languages,
                tools,
            });
        }
    }
    Ok(runs)
}

/// A cache directory name for a document path: its file stem plus a short hash of the path
/// (FNV-1a, so the name is the same on every toolchain; `DefaultHasher` is not).
pub fn stable_key(main: &Path) -> String {
    format!(
        "{}-{}",
        main.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        run::path_key(main)
    )
}

/// The pure computation.
pub fn plan(db: &Tlpdb, runs: &[DocRun], opts: &PruneOptions) -> Plan {
    let mut needed: BTreeMap<String, KeepReason> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut documents = Vec::new();
    // 1. What the documents proved, first reason wins.
    for r in runs {
        let doc = r.main.display().to_string();
        documents.push(DocSummary {
            main: r.main.clone(),
            engine: r.engine,
            packages_used: r.packages.packages.iter().filter(|p| matches!(p.reason, Reason::Used { .. })).count(),
        });
        for p in &r.packages.packages {
            let reason = match &p.reason {
                Reason::Used { file, via } => {
                    KeepReason::Document { doc: doc.clone(), file: file.display().to_string(), via: via.clone() }
                }
                Reason::Infra => KeepReason::Infra { note: "TeX Live machinery" },
                Reason::Engine => KeepReason::Engine { doc: doc.clone() },
                Reason::Format { format } => KeepReason::Format { format: format.clone() },
                Reason::Depend { of } => KeepReason::Depend { of: of.clone() },
            };
            // A file read by any document beats "dependency of something" from an earlier one.
            match needed.get(&p.name) {
                Some(KeepReason::Depend { .. }) if matches!(reason, KeepReason::Document { .. }) => {
                    needed.insert(p.name.clone(), reason);
                }
                Some(_) => {}
                None => {
                    needed.insert(p.name.clone(), reason);
                }
            }
        }
        if !r.packages.unresolved.is_empty() {
            warnings.push(format!(
                "{}: {} texmf-dist files are not in the database (local edits?)",
                doc,
                r.packages.unresolved.len()
            ));
        }
        if !r.packages.local.is_empty() {
            warnings.push(format!(
                "{}: {} files from TEXMFLOCAL/TEXMFHOME are not tlmgr-managed; you keep them yourself",
                doc,
                r.packages.local.len()
            ));
        }
        if r.latexmk {
            needed.entry("latexmk".into()).or_insert(KeepReason::Latexmk);
        }
        for t in &r.tools {
            if db.packages.contains_key(t) {
                needed.entry(t.clone()).or_insert(KeepReason::Tool { doc: doc.clone() });
            } else {
                warnings.push(format!("{doc}: needs {t}, which is not in the database"));
            }
        }
        // Q7: OpenType fonts on XeTeX/LuaTeX.
        if matches!(r.engine, EngineCmd::Xelatex | EngineCmd::Lualatex) {
            let mut unmatched = Vec::new();
            for f in r.pdf_fonts.iter().filter(|f| f.kind != "Type 3") {
                let providers = keep::opentype_providers(db, &f.name);
                if providers.is_empty() {
                    // A Type 1 font through the map file is already proved by the .fls (pdfTeX-style loading).
                    if !f.kind.starts_with("Type 1") {
                        unmatched.push(f.name.clone());
                    }
                } else {
                    for p in providers {
                        needed.entry(p).or_insert(KeepReason::OpenTypeProvider { font: f.name.clone() });
                    }
                }
            }
            if !unmatched.is_empty() {
                let all = keep::all_opentype_providers(db);
                warnings.push(format!(
                    "{}: {} font{} in the PDF could not be matched to a TeX Live package ({}); keeping all {} OpenType/TrueType font packages (Q7, v1 behavior)",
                    doc,
                    unmatched.len(),
                    if unmatched.len() == 1 { "" } else { "s" },
                    unmatched.join(", "),
                    all.len()
                ));
                for p in all {
                    needed
                        .entry(p)
                        .or_insert(KeepReason::OpenTypeProvider { font: "(unmatched; all providers kept)".into() });
                }
            }
        }
    }
    // 2. Infrastructure, hyphenation, user keeps.
    for (name, reason) in keep::infra() {
        needed.entry(name.to_string()).or_insert(reason);
    }
    let langs: Vec<String> = runs.iter().flat_map(|r| r.languages.iter().cloned()).collect();
    for (name, reason) in keep::hyphenation(db, &langs, opts.keep_hyphenation == Hyphenation::All) {
        needed.entry(name).or_insert(reason);
    }
    for k in &opts.keep {
        needed.insert(k.clone(), KeepReason::User);
    }
    // 3. Close over depend, remembering who pulled each one in.
    let roots: Vec<String> = needed.keys().cloned().collect();
    let root_closures: Vec<(&String, BTreeSet<String>)> = roots.iter().map(|r| (r, db.closure([r.as_str()]))).collect();
    for name in db.closure(roots.iter().map(String::as_str)) {
        if let std::collections::btree_map::Entry::Vacant(e) = needed.entry(name.clone()) {
            let of =
                root_closures.iter().find(|(_, c)| c.contains(&name)).map(|(r, _)| (*r).clone()).unwrap_or_default();
            e.insert(KeepReason::Depend { of });
        }
    }
    for k in &opts.keep {
        if !db.packages.contains_key(k) {
            warnings.push(format!("--keep {k}: not an installed package"));
        }
    }
    // 4. Installed minus needed.
    let installed: Vec<&str> = db
        .packages
        .values()
        .filter(|p| {
            !p.name.starts_with("00texlive.")
                && matches!(
                    p.category,
                    Some(
                        Category::Package
                            | Category::TLCore
                            | Category::ConTeXt
                            | Category::Collection
                            | Category::Scheme
                    )
                )
        })
        .map(|p| p.name.as_str())
        .collect();
    let installed_size: u64 = installed.iter().filter_map(|n| db.packages.get(*n)).map(|p| p.run_size).sum();
    let needed_size: u64 = needed.keys().filter_map(|n| db.packages.get(n)).map(|p| p.run_size).sum();
    let mut schemes = Vec::new();
    let mut collections = Vec::new();
    let mut rest = Vec::new();
    for name in installed {
        if needed.contains_key(name) {
            continue;
        }
        match db.packages[name].category {
            Some(Category::Scheme) => schemes.push(name.to_string()),
            Some(Category::Collection) => collections.push(name.to_string()),
            _ => rest.push(name.to_string()),
        }
    }
    // Every collection depends on collection-basic and schemes on collections: dependents first
    // here too, or tlmgr refuses the base while its dependents are still installed.
    let schemes = reverse_dependency_order(db, schemes);
    let collections = reverse_dependency_order(db, collections);
    // tlmgr removes a package's binary stanza with it and refuses the stanza on its own
    // ("needed by <base>"); list a stanza only when its base package stays.
    let removable_set: HashSet<&str> = rest.iter().map(String::as_str).collect();
    let rest: Vec<String> =
        rest.iter().filter(|n| arch_base(n).is_none_or(|base| !removable_set.contains(base))).cloned().collect();
    let removable = reverse_dependency_order(db, rest.clone());
    let mut meta = schemes.clone();
    meta.extend(collections.iter().cloned());
    // Packages can depend on collections (yb-book → collection-fontsextra → collection-basic),
    // so the removal order is one dependents-first order over everything.
    let order = reverse_dependency_order(db, schemes.iter().chain(&collections).chain(&rest).cloned().collect());
    let removable_size: u64 = removable.iter().filter_map(|n| db.packages.get(n)).map(|p| p.run_size).sum();
    Plan {
        schema: 1,
        documents,
        installed: installed_size_count(db),
        installed_size,
        needed,
        needed_size,
        schemes,
        collections,
        meta,
        removable,
        order,
        removable_size,
        warnings,
    }
}

fn installed_size_count(db: &Tlpdb) -> usize {
    db.packages
        .values()
        .filter(|p| {
            !p.name.starts_with("00texlive.")
                && matches!(
                    p.category,
                    Some(
                        Category::Package
                            | Category::TLCore
                            | Category::ConTeXt
                            | Category::Collection
                            | Category::Scheme
                    )
                )
        })
        .count()
}

/// Dependents before dependencies, so `tlmgr remove` never sees a package another
/// still-installed package depends on. Ties keep alphabetical order.
fn reverse_dependency_order(db: &Tlpdb, mut names: Vec<String>) -> Vec<String> {
    names.sort();
    let set: HashSet<&str> = names.iter().map(String::as_str).collect();
    // dependents[x] = removable packages that depend on x
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &names {
        if let Some(p) = db.packages.get(n) {
            for d in &p.depends {
                let d = d.strip_suffix(".ARCH").map(|b| format!("{b}.")).unwrap_or_else(|| d.clone());
                for target in set.iter().filter(|t| **t == d.as_str() || (d.ends_with('.') && t.starts_with(&d))) {
                    dependents.entry(target).or_default().push(n.as_str());
                }
            }
        }
    }
    let mut emitted: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    // Repeated passes: emit a package once every removable dependent is out. Cycles (rare,
    // tlpdb has a few) break by emitting the first remaining name.
    while emitted.len() < names.len() {
        let mut progressed = false;
        for n in &names {
            if emitted.contains(n.as_str()) {
                continue;
            }
            let ready = dependents.get(n.as_str()).is_none_or(|ds| ds.iter().all(|d| emitted.contains(d)));
            if ready {
                emitted.insert(n.as_str());
                out.push(n.clone());
                progressed = true;
            }
        }
        if !progressed && let Some(n) = names.iter().find(|n| !emitted.contains(n.as_str())) {
            emitted.insert(n.as_str());
            out.push(n.clone());
        }
    }
    out
}

/// `keep.txt`: the packages to install on a fresh `scheme-infraonly` (base names only;
/// tlmgr adds the binary stanzas itself).
pub fn keep_list(plan: &Plan) -> String {
    plan.needed
        .keys()
        .filter(|n| !n.contains('.') || n.starts_with("texlive.infra") || n.ends_with(".infra"))
        .filter(|n| !is_arch(n))
        .map(|n| format!("{n}\n"))
        .collect()
}

fn is_arch(name: &str) -> bool {
    arch_base(name).is_some()
}

/// `a2ping.x86_64-linux` → `a2ping`; `None` for a base package (`texlive.infra` has a dot
/// but no `-` after it).
fn arch_base(name: &str) -> Option<&str> {
    name.rsplit_once('.')
        .filter(|(_, a)| a.contains('-') && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map(|(b, _)| b)
}

/// An `install-tl` profile that reproduces the pruned installation additively.
pub fn install_profile(plan: &Plan) -> String {
    format!(
        "# prumo install-tl profile: scheme-infraonly, then `tlmgr install $(cat keep.txt)`\n\
         # {} packages, {} MB\n\
         selected_scheme scheme-infraonly\n\
         instopt_adjustpath 0\n\
         instopt_letter 0\n\
         tlpdbopt_install_docfiles 0\n\
         tlpdbopt_install_srcfiles 0\n",
        plan.needed.len(),
        plan.needed_size / 1_000_000
    )
}

/// The `tlmgr` command lines of the plan, in order, `batch` packages per line.
pub fn commands(plan: &Plan, batch: usize) -> Vec<Vec<String>> {
    let mut cmds = Vec::new();
    for chunk in plan.order.chunks(batch.max(1)) {
        let mut c = vec!["tlmgr".to_string(), "remove".to_string(), "--no-depends".to_string()];
        c.extend(chunk.iter().cloned());
        cmds.push(c);
    }
    cmds
}

/// Write `prune-plan.txt`, `keep.txt` and `prumo.profile` under `out`.
pub fn write_outputs(plan: &Plan, out: &Path) -> Result<(), PruneError> {
    std::fs::create_dir_all(out).map_err(|source| PruneError::Io {
        what: "create",
        path: out.to_path_buf(),
        source,
    })?;
    let w = |name: &str, text: String| {
        let p = out.join(name);
        std::fs::write(&p, text).map_err(|source| PruneError::Io { what: "write", path: p.clone(), source })
    };
    let mut plan_txt = String::new();
    for c in commands(plan, 200) {
        plan_txt.push_str(&c.join(" "));
        plan_txt.push('\n');
    }
    w("prune-plan.txt", plan_txt)?;
    w("keep.txt", keep_list(plan))?;
    w("prumo.profile", install_profile(plan))?;
    Ok(())
}

pub fn text(plan: &Plan, out: &Path) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let mut by_engine: BTreeMap<&str, usize> = BTreeMap::new();
    for d in &plan.documents {
        *by_engine.entry(d.engine.command()).or_default() += 1;
    }
    let engines = by_engine.iter().map(|(e, n)| format!("{e} {n}")).collect::<Vec<_>>().join(", ");
    let _ = writeln!(
        s,
        "== prumo prune: {} document build{} ({engines})",
        plan.documents.len(),
        if plan.documents.len() == 1 { "" } else { "s" }
    );
    let _ = writeln!(
        s,
        "Installed: {} packages ({} MB)   Needed: {} ({} MB)   Removable: {} ({} MB)",
        plan.installed,
        plan.installed_size / 1_000_000,
        plan.needed.len(),
        plan.needed_size / 1_000_000,
        plan.meta.len() + plan.removable.len(),
        plan.removable_size / 1_000_000
    );
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for r in plan.needed.values() {
        let k = match r {
            KeepReason::Document { .. } => "read by a document",
            KeepReason::Infra { .. } => "infrastructure",
            KeepReason::Engine { .. } => "engine",
            KeepReason::Format { .. } => "format and its triggers",
            KeepReason::Hyphenation { .. } => "hyphenation",
            KeepReason::User => "--keep",
            KeepReason::OpenTypeProvider { .. } => "OpenType provider",
            KeepReason::Latexmk => "latexmk",
            KeepReason::Tool { .. } => "build tool (biber, bibtex, makeindex)",
            KeepReason::Depend { .. } => "dependency of a kept package",
        };
        *reasons.entry(k.to_string()).or_default() += 1;
    }
    let _ =
        writeln!(s, "Kept because: {}", reasons.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", "));
    for w in &plan.warnings {
        let _ = writeln!(s, "  warning: {w}");
    }
    let _ = writeln!(s, "\nPlan (dry run — nothing was changed):");
    if !plan.meta.is_empty() {
        let shown: Vec<&str> = plan.meta.iter().take(6).map(String::as_str).collect();
        let _ = writeln!(
            s,
            "  tlmgr remove --no-depends {}{}  ({} meta-packages: schemes and collections)",
            shown.join(" "),
            if plan.meta.len() > 6 { " ..." } else { "" },
            plan.meta.len()
        );
    }
    if !plan.removable.is_empty() {
        let shown: Vec<&str> = plan.removable.iter().take(6).map(String::as_str).collect();
        let _ = writeln!(
            s,
            "  tlmgr remove --no-depends {} ...  ({} packages, dependents first; written to {})",
            shown.join(" "),
            plan.removable.len(),
            crate::doctor::host_path(&out.join("prune-plan.txt"))
        );
    }
    let _ = writeln!(
        s,
        "Keep list: {} (use with `tlmgr install $(cat keep.txt)` on a fresh scheme-infraonly; profile in {})",
        crate::doctor::host_path(&out.join("keep.txt")),
        crate::doctor::host_path(&out.join("prumo.profile"))
    );
    let _ = writeln!(
        s,
        "Apply with `prumo prune ... --apply --yes` after checking the plan; every tlmgr command is logged to {}.",
        crate::doctor::host_path(&out.join("prune.log"))
    );
    s
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApplyReport {
    pub commands_run: usize,
    /// Packages tlmgr reported as removed.
    pub removed: usize,
    /// Packages tlmgr still refused after retries (`not removing X, needed by Y`).
    pub refused: Vec<String>,
    pub log: PathBuf,
}

/// Run the plan's `tlmgr remove` commands, logging everything. Requires `yes`.
///
/// tlmgr refuses a package while an installed scheme, collection or package lists it as a
/// dependency, exits non-zero for the batch, and removes the rest. So: everything in
/// dependents-first order, parse `not removing X`, retry those until no progress.
pub fn apply(plan: &Plan, out: &Path, yes: bool, mut progress: impl FnMut(&str)) -> Result<ApplyReport, PruneError> {
    if !yes {
        return Err(PruneError::NeedsYes);
    }
    if crate::doctor::which("tlmgr").is_none() {
        return Err(PruneError::NoTlmgr);
    }
    std::fs::create_dir_all(out).map_err(|source| PruneError::Io {
        what: "create",
        path: out.to_path_buf(),
        source,
    })?;
    let log_path = out.join("prune.log");
    let mut log = String::new();
    let mut commands_run = 0;
    let mut removed = 0;
    let mut refused: Vec<String> = Vec::new();
    let mut run_batch = |pkgs: &[String], label: &str, log: &mut String| -> Result<Vec<String>, PruneError> {
        let mut argv = vec!["remove".to_string(), "--no-depends".to_string()];
        argv.extend(pkgs.iter().cloned());
        progress(&format!("{label}: tlmgr remove {} packages", pkgs.len()));
        log.push_str(&format!("$ tlmgr {}\n", argv.join(" ")));
        let outp = Command::new("tlmgr").args(&argv).output().map_err(|source| PruneError::Io {
            what: "run",
            path: PathBuf::from("tlmgr"),
            source,
        })?;
        let text = format!("{}{}", String::from_utf8_lossy(&outp.stdout), String::from_utf8_lossy(&outp.stderr));
        log.push_str(&text);
        log.push_str(&format!(
            "# exit {}\n\n",
            outp.status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
        ));
        std::fs::write(&log_path, &*log).map_err(|source| PruneError::Io {
            what: "write",
            path: log_path.clone(),
            source,
        })?;
        commands_run += 1;
        // tlmgr says "not removing" per package and still exits 0; a non-zero exit is a
        // different failure (permissions, a broken tlpdb) and must not read as "nothing left".
        if !outp.status.success() {
            return Err(PruneError::Tlmgr { code: outp.status.code(), log: log_path.clone() });
        }
        if let Some(line) = text.lines().find_map(|l| l.strip_prefix("tlmgr: ultimately removed these packages: ")) {
            removed += line.split_whitespace().count();
        }
        Ok(text
            .lines()
            .filter_map(|l| l.strip_prefix("tlmgr: not removing "))
            .filter_map(|r| r.split(',').next())
            .map(|n| n.trim().to_string())
            .collect())
    };
    // Each group until no progress: tlmgr judges every package against what is installed
    // when it gets to it, so a dependent later in the same call still blocks its dependency.
    let mut pending: Vec<String> = plan.order.clone();
    let mut round = 0;
    while !pending.is_empty() {
        round += 1;
        let mut still = Vec::new();
        for chunk in pending.chunks(200) {
            let r = run_batch(chunk, &format!("round {round}"), &mut log)?;
            still.extend(r.into_iter().filter(|n| chunk.contains(n)));
        }
        if still.len() >= pending.len() || round > 8 {
            refused.extend(still);
            break;
        }
        pending = still;
    }
    refused.sort();
    refused.dedup();
    Ok(ApplyReport { commands_run, removed, refused, log: log_path })
}
