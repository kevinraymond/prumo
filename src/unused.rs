//! `prumo unused`: which `\usepackage` lines can go. Experimental, not static: block the
//! package at the kernel level, rebuild, compare. See `docs/06-unused-packages.md`.
//!
//! Candidates come from the probe tree: packages loaded directly by the document or by the
//! user's own class/package (anything outside `texmf-dist`). Dependencies pulled in by
//! distribution packages are never candidates. Blocking defines the kernel's "already
//! loaded" markers before the document runs:
//!
//! ```text
//! \makeatletter\@namedef{ver@xcolor.sty}{9999/12/31 blocked by prumo}\@namedef{opt@xcolor.sty}{<opts>}\makeatother
//! ```
//!
//! so every `\usepackage{xcolor}` — in the preamble, the class, another package — is a no-op.

use crate::fls::{self, Tree, Trees};
use crate::model::{BuildModel, Frame, FrameKind};
use crate::run::{self, EngineCmd, RunOptions, RunOutput};
use crate::texlog;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Packages that act by side effect and always pass a no-change test while obviously doing
/// something. Reported as "not tested" unless `--test-all`. Keep short and editable.
pub(crate) const SKIP_LIST: [&str; 17] = [
    "fontenc",
    "inputenc",
    "babel",
    "polyglossia",
    "fontspec",
    "microtype",
    "geometry",
    "hyperref",
    "bookmark",
    "lmodern",
    "etoolbox",
    "xparse",
    "expl3",
    "nag",
    "showframe",
    "layout",
    "unicode-math",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Compare {
    /// Page count, `pdftotext -layout` text, and a coarse ink check (24 dpi gray raster,
    /// 0.5 % of pixels may differ) so a package that only draws is not called unused.
    Text,
    /// Text, then a 50 dpi raster compared pixel by pixel (0.1 % tolerance): catches
    /// geometry/microtype-class effects, ~10× slower.
    Render,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "verdict", rename_all = "kebab-case")]
pub enum Verdict {
    /// The build failed, or produced an error the baseline did not have.
    Required { evidence: String },
    /// Page count, text, or rendered pages differ.
    AffectsOutput { evidence: String },
    /// Text identical; `pdfinfo` fields differ.
    AffectsMetadata { evidence: String },
    /// Everything compared is identical: safe to remove, with the standard caveat.
    NoObservableEffect,
    /// Everything compared is identical because another package loaded it anyway
    /// (`graphicx` under `tikz`): the document may well use it, but the line is redundant.
    LoadedAnyway { by: String },
    /// On the skip list.
    NotTested { reason: String },
    /// The candidate build timed out.
    TimedOut,
    /// A distribution unit had already loaded it when the `\usepackage` line ran
    /// (`amsmath` under beamer): the line is redundant. Nothing was rebuilt.
    AlreadyLoaded { by: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    pub package: String,
    pub options: String,
    /// Who loads it: `main.tex`, or `obra.cls` for class-level candidates.
    pub loaded_by: String,
    pub verdict: Verdict,
    /// Seconds the candidate build took, when it ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_secs: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnusedReport {
    pub schema: u32,
    pub job: String,
    pub engine: EngineCmd,
    pub compare: Compare,
    pub baseline_pages: usize,
    pub baseline_secs: f64,
    pub candidates: Vec<Candidate>,
    /// From the cache rather than rebuilt (sources unchanged).
    pub cached: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UnusedOptions {
    pub main: PathBuf,
    pub engine: EngineCmd,
    pub latexmk: bool,
    /// The user's own build command (`--build`); overrides `engine` and `latexmk`.
    pub build: Option<String>,
    pub jobs: usize,
    pub compare: Compare,
    pub test_all: bool,
    /// Per candidate build, in seconds; 0 = 10× the baseline.
    pub timeout_secs: u64,
    pub out: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum UnusedError {
    #[error(transparent)]
    Run(#[from] run::RunError),
    #[error(transparent)]
    Corpus(#[from] crate::prune::PruneError),
    #[error("{0}: not a file; --corpus judges one of your own .cls/.sty files against the documents")]
    NoLoader(PathBuf),
    #[error(transparent)]
    Model(#[from] crate::model::ModelError),
    #[error("the baseline build failed ({}); fix the document first:\n{tail}", run::exit_text(*.code))]
    BaselineFailed { code: Option<i32>, tail: String },
    #[error("pdftotext is not on PATH (poppler-utils); the unused pass compares page text")]
    NoPdftotext,
    #[error("cannot {what} {path}: {source}")]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A package a project file asks for: what `unused` tests one at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub package: String,
    pub options: String,
    /// The project file that asked (`main.tex`, `obra.cls`).
    pub loaded_by: String,
}

/// Candidates: `pkg` frames whose parent is the main file or a project-tree class/package.
pub fn candidates(model: &BuildModel, project_units: &BTreeSet<String>) -> Vec<Request> {
    candidates_in(model, project_units, &BTreeMap::new())
}

/// Package names a LaTeX source asks for in `\usepackage`/`\RequirePackage` lines
/// (comments stripped; options skipped; `{a,b}` split).
pub(crate) fn requested_packages(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let text: String = source.lines().map(|l| l.split('%').next().unwrap_or("")).collect::<Vec<_>>().join("\n");
    for key in ["\\usepackage", "\\RequirePackage"] {
        let mut rest = text.as_str();
        while let Some(i) = rest.find(key) {
            rest = &rest[i + key.len()..];
            let r = rest.trim_start();
            let r = if let Some(r) = r.strip_prefix('[') { r.split_once(']').map(|(_, a)| a).unwrap_or("") } else { r };
            let r = r.trim_start();
            if let Some(r) = r.strip_prefix('{')
                && let Some((names, _)) = r.split_once('}')
            {
                for n in names.split(',') {
                    let n = n.trim();
                    if !n.is_empty() {
                        out.insert(n.to_string());
                    }
                }
            }
        }
    }
    out
}

/// One package load in the tree, with the package that loaded it.
struct Load {
    name: String,
    options: String,
    /// The unit whose file loads it: `main.tex`, `obra.cls`, `beamerbaserequires.sty`.
    parent: String,
    /// The parent is the document or one of its own class/package files.
    parent_is_project: bool,
    /// For a load inside the distribution: the distribution unit the project loaded that
    /// (transitively) caused it, e.g. `beamer.cls` for `beamer → beamerbaserequires → amsmath`.
    top: Option<String>,
    start: u64,
}

/// Every package load in the preamble, first load of each name only. Packages loaded after
/// `\begin{document}` under the main file come from other packages' `\AtBeginDocument`
/// hooks (graphicx → color, epstopdf-base), not from a `\usepackage` line. `predoc`
/// (begindocument/before) is the end of the preamble; older records only have `begindoc`,
/// which comes after other packages' begindocument/before loads.
fn preamble_loads(model: &BuildModel, project_units: &BTreeSet<String>) -> Vec<Load> {
    let begindoc = model.phases.predoc.or(model.phases.begindoc).unwrap_or(u64::MAX);
    loads_before(model, project_units, begindoc)
}

/// Every package load that starts before `cutoff`, first load of each name only.
fn loads_before(model: &BuildModel, project_units: &BTreeSet<String>, begindoc: u64) -> Vec<Load> {
    fn go(
        fr: &Frame,
        parent: &str,
        parent_is_project: bool,
        top: Option<&str>,
        out: &mut Vec<Load>,
        project_units: &BTreeSet<String>,
        begindoc: u64,
    ) {
        for c in &fr.children {
            let label = match c.kind {
                FrameKind::Cls => format!("{}.cls", c.name),
                FrameKind::Pkg => format!("{}.sty", c.name),
                _ => c.name.clone(),
            };
            if c.kind == FrameKind::Pkg && c.start < begindoc {
                out.push(Load {
                    name: c.name.clone(),
                    options: c.options.clone().unwrap_or_default(),
                    parent: parent.to_string(),
                    parent_is_project,
                    top: top.map(str::to_string),
                    start: c.start,
                });
            }
            let is_project = match c.kind {
                FrameKind::File => c.name.ends_with(".tex") && parent_is_project || fr.kind == FrameKind::Job,
                FrameKind::Cls | FrameKind::Pkg => project_units.contains(&label),
                FrameKind::Job => true,
            };
            let top = if is_project {
                None
            } else if parent_is_project {
                Some(label.as_str())
            } else {
                top
            };
            go(c, &label, is_project, top, out, project_units, begindoc);
        }
    }
    let mut out = Vec::new();
    go(&model.root, "the command line", true, None, &mut out, project_units, begindoc);
    let mut seen = BTreeSet::new();
    out.retain(|l| seen.insert(l.name.clone()));
    out
}

impl Load {
    /// Who to blame for a load inside the distribution: the unit the project loaded
    /// (`beamer.cls`), with the direct loader when it differs (`beamer.cls via
    /// beamerbaserequires.sty`); for a project load, the loader itself.
    fn loader_label(&self) -> String {
        match &self.top {
            Some(top) if *top != self.parent => format!("{top} via {}", self.parent),
            Some(top) => top.clone(),
            None => self.parent.clone(),
        }
    }
}

/// After a candidate build whose output did not change: is `pkg` still in the load graph,
/// pulled in by some other package (`graphicx` by `tikz`)? Then the document may use it
/// all the same and the line is redundant rather than the package unused. The blocking
/// fallback leaves no `pkg` frame, so it never answers here.
pub(crate) fn loaded_anyway(model: &BuildModel, project_units: &BTreeSet<String>, pkg: &str) -> Option<String> {
    loads_before(model, project_units, u64::MAX)
        .into_iter()
        .find(|l| l.name == pkg)
        .filter(|l| !l.parent_is_project)
        .map(|l| l.loader_label())
}

/// `candidates()` with the loaders' sources: a package loaded under `main.tex` that
/// `main.tex` never asks for came from some package's hook (siunitx loads `color` from
/// `begindocument/before`) and is not the user's line to delete.
pub(crate) fn candidates_in(
    model: &BuildModel,
    project_units: &BTreeSet<String>,
    sources: &BTreeMap<String, String>,
) -> Vec<Request> {
    preamble_loads(model, project_units)
        .into_iter()
        .filter(|l| l.parent_is_project && l.start < u64::MAX)
        .filter(|l| sources.get(&l.parent).is_none_or(|src| requested_packages(src).contains(&l.name)))
        .map(|l| Request { package: l.name, options: l.options, loaded_by: l.parent })
        .collect()
}

/// `\usepackage` lines a project source has for packages the distribution had already loaded
/// (`\usepackage{amsmath}` under `\documentclass{beamer}`): redundant, nothing to rebuild.
/// `(package, requested by, loaded by)`, where "loaded by" is the distribution unit the
/// project loaded (`beamer.cls`), with the direct loader when it differs
/// (`beamer.cls via beamerbaserequires.sty`).
pub(crate) fn redundant_requests(
    model: &BuildModel,
    project_units: &BTreeSet<String>,
    sources: &BTreeMap<String, String>,
) -> Vec<(String, String, String)> {
    let loads = preamble_loads(model, project_units);
    let mut out = Vec::new();
    for (requester, src) in sources {
        for name in requested_packages(src) {
            let Some(l) = loads.iter().find(|l| l.name == name && !l.parent_is_project) else { continue };
            out.push((name, requester.clone(), l.loader_label()));
        }
    }
    out
}

/// Remove every `\usepackage`/`\RequirePackage` request for `pkg` from a LaTeX source: a
/// one-name request disappears, from a `{a,b}` list the name is dropped. Comments are left
/// alone; a request split over several lines is not found. `None` when there is none.
pub(crate) fn remove_request(source: &str, pkg: &str) -> Option<String> {
    let mut out = String::with_capacity(source.len());
    let mut changed = false;
    for line in source.split_inclusive('\n') {
        let cut = comment_start(line);
        let (code, comment) = line.split_at(cut);
        out.push_str(&remove_in_code(code, pkg, &mut changed));
        out.push_str(comment);
    }
    changed.then_some(out)
}

/// Byte offset of the first `%` that is not escaped, or the line length.
fn comment_start(line: &str) -> usize {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'%' => return i,
            _ => i += 1,
        }
    }
    b.len()
}

fn remove_in_code(code: &str, pkg: &str, changed: &mut bool) -> String {
    let mut out = String::new();
    let mut rest = code;
    'scan: while !rest.is_empty() {
        let Some((i, key)) = ["\\usepackage", "\\RequirePackage"]
            .iter()
            .filter_map(|k| rest.find(k).map(|i| (i, *k)))
            .min_by_key(|(i, _)| *i)
        else {
            break;
        };
        // The command must end there (not \usepackages); then optional [opts] and {names}.
        let after_key = &rest[i + key.len()..];
        if after_key.starts_with(|c: char| c.is_ascii_alphabetic()) {
            out.push_str(&rest[..i + key.len()]);
            rest = after_key;
            continue;
        }
        let mut j = i + key.len();
        let ws = |s: &str| s.len() - s.trim_start().len();
        j += ws(&rest[j..]);
        if rest[j..].starts_with('[') {
            match rest[j..].find(']') {
                Some(e) => j += e + 1,
                None => break,
            }
            j += ws(&rest[j..]);
        }
        if !rest[j..].starts_with('{') {
            out.push_str(&rest[..j]);
            rest = &rest[j..];
            continue 'scan;
        }
        let Some(close) = rest[j..].find('}') else { break };
        let names = &rest[j + 1..j + close];
        let end = j + close + 1;
        let list: Vec<&str> = names.split(',').map(str::trim).collect();
        if !list.contains(&pkg) {
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        *changed = true;
        let kept: Vec<&str> = list.into_iter().filter(|n| *n != pkg).collect();
        out.push_str(&rest[..i]);
        if !kept.is_empty() {
            out.push_str(&rest[i..j + 1]);
            out.push_str(&kept.join(","));
            out.push('}');
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The blocking preamble for one candidate: the fallback when the request is not found in
/// the loader's source (a request split over lines, or generated). The marker carries a
/// far-future date so a package that checks `\@ifpackagelater{<candidate>}{...}` does not
/// fail with "outdated". Blocking has a blind spot that removing the request does not: a
/// package that adapts to the candidate's presence (biblatex to csquotes) sees the marker
/// and uses macros that are not there.
pub(crate) fn blocking_pretex(package: &str, options: &str) -> String {
    format!(
        "\\makeatletter\\@namedef{{ver@{package}.sty}}{{9999/12/31 blocked by prumo}}\\@namedef{{opt@{package}.sty}}{{{options}}}\\makeatother"
    )
}

fn pdftotext(pdf: &Path) -> Option<String> {
    let out = Command::new("pdftotext").args(["-layout", pdf.to_str()?, "-"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `pdfinfo` without the fields that change on every build.
fn pdfinfo(pdf: &Path) -> Option<BTreeMap<String, String>> {
    let out = Command::new("pdfinfo").arg(pdf).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(
        text.lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .filter(|(k, _)| !matches!(k.as_str(), "CreationDate" | "ModDate" | "File size" | "Optimized"))
            .collect(),
    )
}

/// Pages rasterized to gray at `dpi`, as PGM pixel buffers (one per page).
fn render(pdf: &Path, dpi: u32) -> Option<Vec<Vec<u8>>> {
    let dir = tempfile::tempdir().ok()?;
    let prefix = dir.path().join("p");
    let ok =
        Command::new("pdftoppm").args(["-r", &dpi.to_string(), "-gray"]).arg(pdf).arg(&prefix).status().ok()?.success();
    if !ok {
        return None;
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.path()).ok()?.flatten().map(|e| e.path()).collect();
    files.sort();
    let mut pages = Vec::new();
    for f in files {
        let bytes = std::fs::read(&f).ok()?;
        // P5 header: "P5\n<w> <h>\n255\n" then pixels.
        let mut fields = 0;
        let mut i = 0;
        while fields < 4 && i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            fields += 1;
        }
        pages.push(bytes[(i + 1).min(bytes.len())..].to_vec());
    }
    Some(pages)
}

/// Coarse ink check in `Text` mode, strict comparison in `Render` mode.
fn raster_dpi(compare: Compare) -> u32 {
    match compare {
        Compare::Text => 24,
        Compare::Render => 50,
    }
}

fn first_diff_line(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}: {:?} vs {:?}", i + 1, x.trim(), y.trim());
        }
    }
    format!("{} vs {} lines", a.lines().count(), b.lines().count())
}

struct Baseline {
    pages: usize,
    text: String,
    info: BTreeMap<String, String>,
    render: Option<Vec<Vec<u8>>>,
    errors: BTreeSet<String>,
}

fn baseline_of(out: &RunOutput, compare: Compare) -> Result<Baseline, UnusedError> {
    let pdf = out
        .pdf
        .as_ref()
        .ok_or_else(|| UnusedError::BaselineFailed { code: out.info.exit_code, tail: "no PDF".into() })?;
    let text = pdftotext(pdf).ok_or(UnusedError::NoPdftotext)?;
    let info = pdfinfo(pdf).unwrap_or_default();
    let pages = info.get("Pages").and_then(|p| p.parse().ok()).unwrap_or(0);
    let errors = texlog::parse_file(&out.log).map(|l| l.errors.into_iter().collect()).unwrap_or_default();
    let render = render(pdf, raster_dpi(compare));
    Ok(Baseline { pages, text, info, render, errors })
}

fn judge(base: &Baseline, out: &RunOutput, compare: Compare) -> Verdict {
    if out.info.exit_code != Some(0) || out.pdf.is_none() {
        let log = texlog::parse_file(&out.log).ok();
        let new_err = log
            .as_ref()
            .and_then(|l| l.errors.iter().find(|e| !base.errors.contains(*e)).cloned())
            .unwrap_or_else(|| format!("exit code {:?}", out.info.exit_code));
        return Verdict::Required { evidence: new_err };
    }
    if let Ok(log) = texlog::parse_file(&out.log)
        && let Some(e) = log.errors.iter().find(|e| !base.errors.contains(*e))
    {
        return Verdict::Required { evidence: e.clone() };
    }
    let pdf = out.pdf.as_ref().expect("checked");
    let Some(text) = pdftotext(pdf) else { return Verdict::Required { evidence: "no readable PDF".into() } };
    let info = pdfinfo(pdf).unwrap_or_default();
    let pages = info.get("Pages").and_then(|p| p.parse().ok()).unwrap_or(0);
    if pages != base.pages {
        return Verdict::AffectsOutput { evidence: format!("{} pages instead of {}", pages, base.pages) };
    }
    if text != base.text {
        return Verdict::AffectsOutput { evidence: format!("text differs at {}", first_diff_line(&base.text, &text)) };
    }
    if let (Some(a), Some(b)) = (&base.render, render(pdf, raster_dpi(compare))) {
        let per_mille = match compare {
            Compare::Text => 5,
            Compare::Render => 1,
        };
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            let differing = x.iter().zip(y).filter(|(p, q)| p.abs_diff(**q) > 32).count();
            if x.len() != y.len() || differing * 1000 > x.len().max(1) * per_mille {
                let what = if compare == Compare::Text { "ink differs" } else { "renders differently" };
                return Verdict::AffectsOutput {
                    evidence: format!(
                        "page {} {what} ({} of {} pixels at {} dpi; text identical)",
                        i + 1,
                        differing,
                        x.len(),
                        raster_dpi(compare)
                    ),
                };
            }
        }
    }
    if info != base.info {
        let k = info
            .keys()
            .chain(base.info.keys())
            .find(|k| info.get(*k) != base.info.get(*k))
            .cloned()
            .unwrap_or_default();
        return Verdict::AffectsMetadata {
            evidence: format!("pdfinfo {k}: {:?} vs {:?}", base.info.get(&k), info.get(&k)),
        };
    }
    Verdict::NoObservableEffect
}

/// Hash of the project-tree sources the baseline read, for the verdict cache.
fn source_hash(out: &RunOutput, main: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let Ok((_, project)) = run::job_of(main) else { return 0 };
    if let Ok(lines) = fls::parse_file(&out.fls)
        && let Some(pwd) = fls::pwd(&lines).map(Path::to_path_buf)
    {
        let trees = Trees { project: Some(pwd.clone()), ..Default::default() };
        for f in fls::input_files(&lines, &trees) {
            if f.tree == Tree::Project
                && let Ok(rel) = f.path.strip_prefix(&pwd)
                && let Ok(bytes) = std::fs::read(project.join(rel))
            {
                rel.hash(&mut h);
                bytes.hash(&mut h);
            }
        }
    }
    h.finish()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheFile {
    source_hash: u64,
    engine: EngineCmd,
    compare: String,
    test_all: bool,
    candidates: Vec<serde_json::Value>,
}

/// The baseline run and what the experiments need from it.
struct BaseRun {
    job: String,
    out_root: PathBuf,
    run: RunOutput,
    model: BuildModel,
    /// The project's own classes and packages, by file name.
    project_units: BTreeSet<String>,
    /// The main file and each project unit, by file name: where the requests are.
    sources: BTreeMap<String, String>,
}

/// The ordinary probed run (byte-identical to a plain build), reused from the cache when it
/// is current, plus the project files that may hold the `\usepackage` lines.
fn base_run(opts: &UnusedOptions, progress: &mut impl FnMut(&str)) -> Result<BaseRun, UnusedError> {
    let (job, project) = run::job_of(&opts.main)?;
    let out_root = match &opts.out {
        Some(o) => o.clone(),
        None => run::default_out(&opts.main)?,
    };
    progress(&format!("artifacts in {}", out_root.join(&job).display()));
    let base_opts = RunOptions {
        latexmk: opts.latexmk,
        build: opts.build.clone(),
        out: Some(out_root.clone()),
        env: opts.env.clone(),
        ..RunOptions::new(opts.main.clone(), opts.engine)
    };
    let run = match run::cached(&opts.main, Some(&out_root))?.filter(|c| !run::is_stale(c, &base_opts)) {
        Some(c) => c,
        None => run::run(&base_opts)?,
    };
    if run.info.exit_code != Some(0) {
        // The first `!` error says why; a log tail usually ends in memory statistics.
        let tail = match crate::texlog::parse_file(&run.log).ok().and_then(|l| l.errors.into_iter().next()) {
            Some(e) => format!("! {e}"),
            None => std::fs::read_to_string(&run.log).map(|t| run::tail_lines(&t, 10)).unwrap_or_default(),
        };
        return Err(UnusedError::BaselineFailed { code: run.info.exit_code, tail });
    }
    let model = run::model_of(&run)?;
    let lines = fls::parse_file(&run.fls).map_err(run::RunError::Fls)?;
    let trees = Trees::from_kpsewhich(&std::fs::read_to_string(run.dir.join("kpsewhich.txt")).unwrap_or_default());
    let project_units: BTreeSet<String> = fls::input_files(&lines, &trees)
        .into_iter()
        .filter(|f| f.tree == Tree::Project && matches!(f.kind, fls::InputKind::Cls | fls::InputKind::Sty))
        .filter_map(|f| f.path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(t) = std::fs::read_to_string(&opts.main)
        && let Some(n) = opts.main.file_name()
    {
        sources.insert(n.to_string_lossy().into_owned(), t);
    }
    for u in &project_units {
        if let Ok(t) = std::fs::read_to_string(project.join(u)) {
            sources.insert(u.clone(), t);
        }
    }
    Ok(BaseRun { job, out_root, run, model, project_units, sources })
}

/// The `compare` mode as the cache file spells it.
fn compare_key(compare: Compare) -> String {
    format!("{compare:?}").to_lowercase()
}

/// The verdicts of an earlier pass over the same sources, engine and mode, when the cache
/// file holds them.
fn cached_candidates(cache_path: &Path, hash: u64, opts: &UnusedOptions) -> Option<Vec<Candidate>> {
    let text = std::fs::read_to_string(cache_path).ok()?;
    let c = serde_json::from_str::<CacheFile>(&text).ok()?;
    let same = c.source_hash == hash
        && c.engine == opts.engine
        && c.compare == compare_key(opts.compare)
        && c.test_all == opts.test_all;
    same.then(|| serde_json::from_value::<Vec<Candidate>>(serde_json::Value::Array(c.candidates)).ok()).flatten()
}

/// One experiment: the document rebuilt without `r`, judged against the baseline. The
/// request is deleted from the loader's source in the copy, the way the user would do it;
/// when it is not found there, the package is blocked instead.
fn try_without(opts: &UnusedOptions, b: &BaseRun, base: &Baseline, r: &Request, timeout: f64) -> Verdict {
    let out_dir = b.out_root.join(&b.job).join("unused").join(&r.package);
    let edit = b
        .sources
        .get(&r.loaded_by)
        .and_then(|src| remove_request(src, &r.package))
        .map(|t| (PathBuf::from(&r.loaded_by), t));
    let (edits, pretex) = match edit {
        Some(e) => (vec![e], None),
        None => (vec![], Some(blocking_pretex(&r.package, &r.options))),
    };
    let result = run::run(&RunOptions {
        latexmk: opts.latexmk,
        build: opts.build.clone(),
        out: Some(out_dir),
        env: opts.env.clone(),
        pretex,
        edits,
        timeout: Some(std::time::Duration::from_secs_f64(timeout)),
        ..RunOptions::new(opts.main.clone(), opts.engine)
    });
    match result {
        Ok(out) => match judge(base, &out, opts.compare) {
            Verdict::NoObservableEffect => {
                let still = run::parse_records(&out)
                    .ok()
                    .and_then(|recs| BuildModel::from_records(&recs, &out.records.display().to_string()).ok())
                    .and_then(|m| loaded_anyway(&m, &b.project_units, &r.package));
                match still {
                    Some(by) => Verdict::LoadedAnyway { by },
                    None => Verdict::NoObservableEffect,
                }
            }
            v => v,
        },
        Err(run::RunError::TimedOut { .. }) => Verdict::TimedOut,
        Err(run::RunError::NoRecords { code, tail, .. }) => Verdict::Required {
            evidence: format!(
                "build produced no records ({}): {}",
                run::exit_text(code),
                tail.lines().last().unwrap_or("")
            ),
        },
        Err(e) => Verdict::Required { evidence: e.to_string() },
    }
}

/// Every candidate through `try_without`, `opts.jobs` at a time. Verdict and wall time,
/// by package name.
fn test_candidates(
    opts: &UnusedOptions,
    b: &BaseRun,
    base: &Baseline,
    to_test: &[&Request],
    timeout: f64,
    progress: impl FnMut(&str) + Send,
) -> BTreeMap<String, (Verdict, f64)> {
    let results: Mutex<BTreeMap<String, (Verdict, f64)>> = Mutex::new(BTreeMap::new());
    let next = AtomicUsize::new(0);
    let progress = Mutex::new(progress);
    std::thread::scope(|s| {
        for _ in 0..opts.jobs.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(r) = to_test.get(i) else { break };
                    let t0 = std::time::Instant::now();
                    let verdict = try_without(opts, b, base, r, timeout);
                    let secs = t0.elapsed().as_secs_f64();
                    (progress.lock().unwrap())(&format!("{}: {} ({secs:.1} s)", r.package, verdict_word(&verdict)));
                    results.lock().unwrap().insert(r.package.clone(), (verdict, secs));
                }
            });
        }
    });
    results.into_inner().unwrap()
}

/// The whole pass. `progress` gets one line per candidate build.
pub fn run(opts: &UnusedOptions, mut progress: impl FnMut(&str) + Send) -> Result<UnusedReport, UnusedError> {
    if crate::doctor::which("pdftotext").is_none() {
        return Err(UnusedError::NoPdftotext);
    }
    let b = base_run(opts, &mut progress)?;
    let baseline_secs = crate::scaled_to_secs(b.model.total());
    let cands = candidates_in(&b.model, &b.project_units, &b.sources);
    let redundant = redundant_requests(&b.model, &b.project_units, &b.sources);
    let base = baseline_of(&b.run, opts.compare)?;
    let report = |candidates: Vec<Candidate>, cached: bool| UnusedReport {
        schema: crate::JSON_SCHEMA,
        job: b.job.clone(),
        engine: opts.engine,
        compare: opts.compare,
        baseline_pages: base.pages,
        baseline_secs,
        candidates,
        cached,
        warnings: vec![],
    };

    // Cache: verdicts for the same sources, engine, mode.
    let hash = source_hash(&b.run, &opts.main);
    let cache_path = b.run.dir.join("unused.json");
    if let Some(candidates) = cached_candidates(&cache_path, hash, opts) {
        return Ok(report(candidates, true));
    }

    let timeout = if opts.timeout_secs > 0 { opts.timeout_secs as f64 } else { (baseline_secs * 10.0).max(30.0) };
    let to_test: Vec<&Request> =
        cands.iter().filter(|r| opts.test_all || !SKIP_LIST.contains(&r.package.as_str())).collect();
    progress(&format!(
        "{} candidates × ~{:.1} s ≈ {:.0} s on {} job{}",
        to_test.len(),
        baseline_secs,
        to_test.len() as f64 * baseline_secs / opts.jobs.max(1) as f64,
        opts.jobs.max(1),
        if opts.jobs == 1 { "" } else { "s" }
    ));
    let results = test_candidates(opts, &b, &base, &to_test, timeout, progress);

    let mut candidates: Vec<Candidate> = cands
        .iter()
        .map(|r| {
            let (verdict, build_secs) = match results.get(&r.package) {
                Some((v, s)) => (v.clone(), Some(*s)),
                None => (
                    Verdict::NotTested { reason: "side-effect package (skip list; --test-all overrides)".into() },
                    None,
                ),
            };
            Candidate {
                package: r.package.clone(),
                options: r.options.clone(),
                loaded_by: r.loaded_by.clone(),
                verdict,
                build_secs,
            }
        })
        .collect();
    candidates.extend(redundant.into_iter().map(|(pkg, requester, by)| Candidate {
        package: pkg,
        options: String::new(),
        loaded_by: requester,
        verdict: Verdict::AlreadyLoaded { by },
        build_secs: None,
    }));
    let cache = CacheFile {
        source_hash: hash,
        engine: opts.engine,
        compare: compare_key(opts.compare),
        test_all: opts.test_all,
        candidates: candidates.iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect(),
    };
    let _ = std::fs::write(&cache_path, serde_json::to_string_pretty(&cache).unwrap_or_default());
    Ok(report(candidates, false))
}

/// `prumo unused --corpus`: one loader (the user's class or package) judged against every
/// document that uses it. A package the loader loads is removable only if no document in the
/// corpus needs it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CorpusReport {
    pub schema: u32,
    /// The loader's file name, e.g. `obra.cls`.
    pub loader: String,
    pub documents: Vec<CorpusDoc>,
    pub packages: Vec<CorpusPackage>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CorpusDoc {
    pub main: PathBuf,
    pub engine: EngineCmd,
    /// The document's build loaded the loader (otherwise it has no say).
    pub uses_loader: bool,
    pub cached: bool,
    /// The document's own pass failed (baseline build broken, …); it has no say either.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CorpusPackage {
    pub package: String,
    pub options: String,
    /// Per document (path as given), the verdict from that document's unused pass.
    pub verdicts: BTreeMap<String, Verdict>,
    /// No document that uses the loader needs it (every verdict is "no observable effect").
    pub removable: bool,
    /// Documents whose build needs it (required, affects output or metadata, timed out).
    pub needed_by: Vec<String>,
    /// Documents where it was not tested (skip list).
    pub untested_in: Vec<String>,
}

/// Run the unused pass on every document and merge the verdicts for `loader`'s packages.
/// `opts.main` is ignored; `opts.out`, when set, roots every document's cache under
/// `<out>/corpus/<engine>/<key>`, otherwise each document's own `.prumo`. `docs` are files,
/// directories or globs, as for `prune`; a `% !TEX program =` magic comment picks the engine.
pub fn run_corpus(
    loader: &Path,
    docs: &[String],
    opts: &UnusedOptions,
    mut progress: impl FnMut(&str) + Send,
) -> Result<CorpusReport, UnusedError> {
    if !loader.is_file() {
        return Err(UnusedError::NoLoader(loader.to_path_buf()));
    }
    let loader_name = loader.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mains = crate::prune::discover(docs)?;
    let mut documents = Vec::new();
    let mut packages: BTreeMap<String, CorpusPackage> = BTreeMap::new();
    let mut warnings = Vec::new();
    for main in mains {
        let engine = crate::prune::magic_engine(&main).unwrap_or(opts.engine);
        let out =
            opts.out.as_ref().map(|o| o.join("corpus").join(engine.command()).join(crate::prune::stable_key(&main)));
        let shown = main.display().to_string();
        progress(&format!("{shown} on {}", engine.command()));
        let r = match run(&UnusedOptions { main: main.clone(), engine, out, ..opts.clone() }, |m| {
            progress(&format!("{shown}: {m}"))
        }) {
            Ok(r) => r,
            Err(UnusedError::NoPdftotext) => return Err(UnusedError::NoPdftotext),
            Err(e) => {
                // One broken document must not sink the corpus: say so and go on.
                let first = e.to_string().lines().next().unwrap_or("failed").to_string();
                warnings.push(format!("{shown}: {first}"));
                documents.push(CorpusDoc { main, engine, uses_loader: false, cached: false, error: Some(first) });
                continue;
            }
        };
        let uses_loader = r.candidates.iter().any(|c| c.loaded_by == loader_name);
        if !uses_loader {
            warnings.push(format!("{shown}: its build does not load {loader_name}; it has no say"));
        }
        for c in r.candidates.iter().filter(|c| c.loaded_by == loader_name) {
            let e = packages.entry(c.package.clone()).or_insert_with(|| CorpusPackage {
                package: c.package.clone(),
                options: c.options.clone(),
                verdicts: BTreeMap::new(),
                removable: false,
                needed_by: vec![],
                untested_in: vec![],
            });
            e.verdicts.insert(shown.clone(), c.verdict.clone());
            match &c.verdict {
                Verdict::NoObservableEffect | Verdict::LoadedAnyway { .. } | Verdict::AlreadyLoaded { .. } => {}
                Verdict::NotTested { .. } => e.untested_in.push(shown.clone()),
                _ => e.needed_by.push(shown.clone()),
            }
        }
        documents.push(CorpusDoc { main, engine, uses_loader, cached: r.cached, error: None });
    }
    for p in packages.values_mut() {
        p.removable = p.needed_by.is_empty() && p.untested_in.is_empty() && !p.verdicts.is_empty();
    }
    if !documents.iter().any(|d| d.uses_loader) {
        warnings.push(format!("no document in the corpus loads {loader_name}"));
    }
    Ok(CorpusReport {
        schema: crate::JSON_SCHEMA,
        loader: loader_name,
        documents,
        packages: packages.into_values().collect(),
        warnings,
    })
}

pub fn corpus_text(r: &CorpusReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let using = r.documents.iter().filter(|d| d.uses_loader).count();
    let _ = writeln!(
        s,
        "== prumo unused --corpus: {} judged against {} document{} ({} load{} it)",
        r.loader,
        r.documents.len(),
        if r.documents.len() == 1 { "" } else { "s" },
        using,
        if using == 1 { "s" } else { "" }
    );
    for w in &r.warnings {
        let _ = writeln!(s, "  note: {w}");
    }
    let w = r.packages.iter().map(|p| p.package.len()).max().unwrap_or(8);
    let _ = writeln!(s, "\n-- packages {} loads --", r.loader);
    for p in &r.packages {
        let status = if p.removable {
            format!("removable (no observable effect in {} of {} documents)", p.verdicts.len(), using)
        } else if !p.needed_by.is_empty() {
            let shown: Vec<String> = p
                .needed_by
                .iter()
                .map(|d| {
                    let short = Path::new(d).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(d.clone());
                    format!("{short} ({})", verdict_word(&p.verdicts[d]))
                })
                .collect();
            format!("needed by {}", shown.join(", "))
        } else {
            format!(
                "not tested in {} document{} (skip list; --test-all)",
                p.untested_in.len(),
                if p.untested_in.len() == 1 { "" } else { "s" }
            )
        };
        let opts = if p.options.is_empty() { String::new() } else { format!("[{}]", p.options) };
        let _ = writeln!(s, "  {:<w$} {:<22} {status}", p.package, opts);
    }
    let removable: Vec<&str> = r.packages.iter().filter(|p| p.removable).map(|p| p.package.as_str()).collect();
    if removable.is_empty() {
        let _ = writeln!(s, "\n  every package {} loads is needed by some document in the corpus.", r.loader);
    } else {
        let _ = writeln!(
            s,
            "\n  {} can drop {} package{}: {}. Caveat: judged on these {} documents' builds only.",
            r.loader,
            removable.len(),
            if removable.len() == 1 { "" } else { "s" },
            removable.join(", "),
            using
        );
    }
    s
}

fn verdict_word(v: &Verdict) -> &'static str {
    match v {
        Verdict::Required { .. } => "required",
        Verdict::AffectsOutput { .. } => "affects output",
        Verdict::AffectsMetadata { .. } => "affects metadata",
        Verdict::NoObservableEffect => "no observable effect",
        Verdict::LoadedAnyway { .. } => "loaded anyway",
        Verdict::NotTested { .. } => "not tested",
        Verdict::TimedOut => "timed out",
        Verdict::AlreadyLoaded { .. } => "already loaded",
    }
}

pub fn text(r: &UnusedReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "== prumo unused: {} ({}, {} pages, baseline {:.1} s, compare {:?}{})",
        r.job,
        r.engine.command(),
        r.baseline_pages,
        r.baseline_secs,
        r.compare,
        if r.cached { ", from cache" } else { "" }
    );
    let w = r.candidates.iter().map(|c| c.package.len()).max().unwrap_or(8);
    let mut by_loader: BTreeMap<&str, Vec<&Candidate>> = BTreeMap::new();
    for c in &r.candidates {
        by_loader.entry(c.loaded_by.as_str()).or_default().push(c);
    }
    for (loader, cs) in &by_loader {
        let _ = writeln!(s, "\n-- loaded by {loader} --");
        for c in cs {
            let evidence = match &c.verdict {
                Verdict::Required { evidence }
                | Verdict::AffectsOutput { evidence }
                | Verdict::AffectsMetadata { evidence } => format!("  ({evidence})"),
                Verdict::NotTested { reason } => format!("  ({reason})"),
                Verdict::AlreadyLoaded { by } => format!("  (by {by} before this line; the line is redundant)"),
                Verdict::LoadedAnyway { by } => format!("  ({by} loads it without this line; the line is redundant)"),
                _ => String::new(),
            };
            let opts = if c.options.is_empty() { String::new() } else { format!("[{}]", c.options) };
            let _ = writeln!(s, "  {:<w$} {:<22} {:<21}{evidence}", c.package, opts, verdict_word(&c.verdict));
        }
    }
    let removable: Vec<&str> =
        r.candidates.iter().filter(|c| c.verdict == Verdict::NoObservableEffect).map(|c| c.package.as_str()).collect();
    if !removable.is_empty() {
        let _ = writeln!(
            s,
            "\n  {} package{} with no observable effect: {}. Caveat: tested on this document's build only (text, page count, {} and metadata); a package can matter for input this document does not exercise.",
            removable.len(),
            if removable.len() == 1 { "" } else { "s" },
            removable.join(", "),
            if r.compare == Compare::Render {
                "a 50 dpi render"
            } else {
                "a coarse ink check; --compare render is stricter"
            }
        );
    }
    let class_level: Vec<&Candidate> = r
        .candidates
        .iter()
        .filter(|c| c.loaded_by.ends_with(".cls") && c.verdict == Verdict::NoObservableEffect)
        .collect();
    if !class_level.is_empty() {
        let _ = writeln!(
            s,
            "  {} loads {} package{} this document does not use: {}. A class serving many documents should be judged against all of them.",
            class_level[0].loaded_by,
            class_level.len(),
            if class_level.len() == 1 { "" } else { "s" },
            class_level.iter().map(|c| c.package.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    let redundant: Vec<&Candidate> = r
        .candidates
        .iter()
        .filter(|c| matches!(c.verdict, Verdict::AlreadyLoaded { .. } | Verdict::LoadedAnyway { .. }))
        .collect();
    if !redundant.is_empty() {
        let _ = writeln!(
            s,
            "  {} \\usepackage line{} redundant: {}. The document may use the package all the same; the line can go because another load supplies it. Harmless unless the options differ from the other load (then LaTeX reports an option clash).",
            redundant.len(),
            if redundant.len() == 1 { " is" } else { "s are" },
            redundant
                .iter()
                .map(|c| format!(
                    "{} ({} by {})",
                    c.package,
                    if matches!(c.verdict, Verdict::AlreadyLoaded { .. }) { "already loaded" } else { "loaded anyway" },
                    match &c.verdict {
                        Verdict::AlreadyLoaded { by } | Verdict::LoadedAnyway { by } => by.as_str(),
                        _ => "",
                    }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if r.candidates.iter().any(|c| matches!(c.verdict, Verdict::NotTested { .. })) {
        let _ = writeln!(
            s,
            "  side-effect packages are not tested by default (they always pass a no-change test): --test-all tests them anyway."
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::parse_str;

    #[test]
    fn candidates_are_document_and_own_class_loads_only() {
        let recs = parse_str(
            "start 0 0 pdftex\nfile+ 0 0 main.tex\ncls+ 1 1 obra\nfile+ 2 1 obra.cls\npkg+ 3 2 siunitx\nopts 3 2 a=b\nfile+ 4 2 siunitx.sty\npkg+ 5 3 translations\nopts 5 3 \nfile+ 6 3 translations.sty\nfile- 7 3 translations.sty\npkg- 8 3 translations\nfile- 9 2 siunitx.sty\npkg- 10 2 siunitx\nfile- 11 1 obra.cls\ncls- 12 1 obra\npkg+ 13 1 graphicx\nopts 13 1 \nfile+ 14 1 graphicx.sty\npkg+ 15 2 graphics\nopts 15 2 \nfile+ 16 2 graphics.sty\nfile- 17 2 graphics.sty\npkg- 18 2 graphics\nfile- 19 1 graphicx.sty\npkg- 20 1 graphicx\nbegindoc 21 1 \npkg+ 22 1 color\nopts 22 1 \nfile+ 23 1 color.sty\nfile- 24 1 color.sty\npkg- 25 1 color\nend 30 0 \n",
            "t",
        )
        .unwrap();
        let m = BuildModel::from_records(&recs, "t").unwrap();
        let c = candidates(&m, &["obra.cls".to_string()].into_iter().collect());
        let names: Vec<(&str, &str, &str)> =
            c.iter().map(|r| (r.package.as_str(), r.options.as_str(), r.loaded_by.as_str())).collect();
        assert_eq!(
            names,
            [("siunitx", "a=b", "obra.cls"), ("graphicx", "", "main.tex")],
            "translations and graphics are distribution dependencies; color is a begin-document hook load"
        );
        let c = candidates(&m, &BTreeSet::new());
        assert_eq!(
            c.iter().map(|x| x.package.as_str()).collect::<Vec<_>>(),
            ["graphicx"],
            "a distribution class's loads are not the user's call"
        );
    }

    #[test]
    fn redundant_requests_under_a_distribution_class() {
        // main.tex → beamer (dist) → beamerbaserequires → amsmath; main.tex asks for amsmath and tikz.
        let recs = parse_str(
            "start 0 0 pdftex\nfile+ 0 0 main.tex\ncls+ 1 1 beamer\nfile+ 2 1 beamer.cls\npkg+ 3 2 beamerbaserequires\nopts 3 2 \nfile+ 4 2 beamerbaserequires.sty\npkg+ 5 3 amsmath\nopts 5 3 \nfile+ 6 3 amsmath.sty\nfile- 7 3 amsmath.sty\npkg- 8 3 amsmath\nfile- 9 2 beamerbaserequires.sty\npkg- 10 2 beamerbaserequires\nfile- 11 1 beamer.cls\ncls- 12 1 beamer\npkg+ 13 1 tikz\nopts 13 1 \nfile+ 14 1 tikz.sty\nfile- 15 1 tikz.sty\npkg- 16 1 tikz\npredoc 17 1 \nbegindoc 18 1 \nend 30 0 \n",
            "t",
        )
        .unwrap();
        let m = BuildModel::from_records(&recs, "t").unwrap();
        let src: BTreeMap<String, String> =
            [("main.tex".to_string(), "\\documentclass{beamer}\\usepackage{amsmath}\\usepackage{tikz}".to_string())]
                .into_iter()
                .collect();
        let none = BTreeSet::new();
        let c = candidates_in(&m, &none, &src);
        assert_eq!(c.iter().map(|x| x.package.as_str()).collect::<Vec<_>>(), ["tikz"], "amsmath is beamer's load");
        let r = redundant_requests(&m, &none, &src);
        assert_eq!(
            r,
            [("amsmath".to_string(), "main.tex".to_string(), "beamer.cls via beamerbaserequires.sty".to_string())]
        );
        // With beamer.cls as the user's own class, the line is still redundant: the class's
        // distribution package loaded amsmath first.
        let mine: BTreeSet<String> = ["beamer.cls".to_string()].into_iter().collect();
        let r = redundant_requests(&m, &mine, &src);
        assert_eq!(r, [("amsmath".to_string(), "main.tex".to_string(), "beamerbaserequires.sty".to_string())]);
    }

    #[test]
    fn requested_packages_from_source() {
        let r = requested_packages(
            "\\documentclass{obra}\n\\usepackage[T1]{fontenc} % \\usepackage{nope}\n\\usepackage{amsmath,xcolor}\n\\RequirePackage [a=b] {geometry}\n",
        );
        assert_eq!(r.into_iter().collect::<Vec<_>>(), ["amsmath", "fontenc", "geometry", "xcolor"]);
    }

    #[test]
    fn sources_filter_hook_loads() {
        let recs = parse_str("start 0 0 pdftex\nfile+ 0 0 main.tex\npkg+ 1 1 siunitx\nopts 1 1 \nfile+ 2 1 siunitx.sty\nfile- 3 1 siunitx.sty\npkg- 4 1 siunitx\npkg+ 5 1 color\nopts 5 1 \nfile+ 6 1 color.sty\nfile- 7 1 color.sty\npkg- 8 1 color\npredoc 9 1 \nbegindoc 10 1 \nend 20 0 \n", "t").unwrap();
        let m = BuildModel::from_records(&recs, "t").unwrap();
        assert_eq!(m.phases.predoc, Some(9));
        let all = candidates(&m, &BTreeSet::new());
        assert_eq!(all.iter().map(|c| c.package.as_str()).collect::<Vec<_>>(), ["siunitx", "color"]);
        let src: BTreeMap<String, String> =
            [("main.tex".to_string(), "\\usepackage{siunitx}".to_string())].into_iter().collect();
        let filtered = candidates_in(&m, &BTreeSet::new(), &src);
        assert_eq!(filtered.iter().map(|c| c.package.as_str()).collect::<Vec<_>>(), ["siunitx"]);
    }

    #[test]
    fn requests_are_removed_from_the_source() {
        let src = "\\documentclass{article}\n\\usepackage[T1]{fontenc} % \\usepackage{xcolor} stays\n\\usepackage{amsmath,xcolor,booktabs}\n\\RequirePackage [x=1] {xcolor}\\usepackage{xcolorx}\n";
        let t = remove_request(src, "xcolor").unwrap();
        assert_eq!(
            t,
            "\\documentclass{article}\n\\usepackage[T1]{fontenc} % \\usepackage{xcolor} stays\n\\usepackage{amsmath,booktabs}\n\\usepackage{xcolorx}\n"
        );
        assert_eq!(remove_request(src, "fontenc").unwrap(), src.replace("\\usepackage[T1]{fontenc} ", " "));
        assert!(remove_request(src, "geometry").is_none());
        assert!(remove_request("\\usepackage{a}\\usepackages{b}", "b").is_none());
        assert_eq!(remove_request("x\\%\\usepackage{a}", "a").unwrap(), "x\\%");
    }

    #[test]
    fn blocking_preamble() {
        assert_eq!(
            blocking_pretex("xcolor", "table"),
            "\\makeatletter\\@namedef{ver@xcolor.sty}{9999/12/31 blocked by prumo}\\@namedef{opt@xcolor.sty}{table}\\makeatother"
        );
    }

    #[test]
    fn first_diff() {
        assert_eq!(first_diff_line("a\nb\nc", "a\nx\nc"), "line 2: \"b\" vs \"x\"");
        assert_eq!(first_diff_line("a\nb", "a\nb\nc"), "2 vs 3 lines");
    }

    #[test]
    fn loaded_anyway_names_the_other_loader() {
        // The candidate build without \usepackage{graphicx}: main.tex → tikz → pgf → graphicx.
        let recs = parse_str(
            "start 0 0 luatex\nfile+ 0 0 main.tex\npkg+ 1 1 tikz\nopts 1 1 \nfile+ 2 1 tikz.sty\npkg+ 3 2 pgf\nopts 3 2 \nfile+ 4 2 pgf.sty\npkg+ 5 3 graphicx\nopts 5 3 \nfile+ 6 3 graphicx.sty\nfile- 7 3 graphicx.sty\npkg- 8 3 graphicx\nfile- 9 2 pgf.sty\npkg- 10 2 pgf\nfile- 11 1 tikz.sty\npkg- 12 1 tikz\npredoc 13 1 \nbegindoc 14 1 \nend 30 0 \n",
            "t",
        )
        .unwrap();
        let m = BuildModel::from_records(&recs, "t").unwrap();
        let none = BTreeSet::new();
        assert_eq!(loaded_anyway(&m, &none, "graphicx").as_deref(), Some("tikz.sty via pgf.sty"));
        assert_eq!(loaded_anyway(&m, &none, "tikz"), None, "a project load is the user's own line");
        assert_eq!(loaded_anyway(&m, &none, "xcolor"), None, "not in the graph at all");
        // With tikz.sty as the user's own package, pgf.sty is the unit to blame.
        let mine: BTreeSet<String> = ["tikz.sty".to_string()].into_iter().collect();
        assert_eq!(loaded_anyway(&m, &mine, "graphicx").as_deref(), Some("pgf.sty"));
    }
}
