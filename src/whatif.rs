//! `prumo whatif`: measured speed-up experiments. Each experiment rebuilds the document with
//! one change and times it against the plain build. Everything runs in a temp copy; the
//! user's files are never touched.
//!
//! - `preamble`: the preamble precompiled into a format with mylatexformat
//!   (`pdftex -ini -jobname=<job>-fmt "&pdflatex" mylatexformat.ltx <job>.tex`, then
//!   `pdflatex -fmt=<job>-fmt <job>.tex`). pdfTeX only in practice: XeTeX cannot dump the
//!   OpenType font the class loads, and LuaTeX's dump fails in lua-uni-algos (TeX Live 2026).
//! - `draft`: no PDF (`-draftmode`, `--draftmode`, `-no-pdf`), which is what writing the PDF
//!   and including images cost.
//! - `externalize`: each tikzpicture built once into its own PDF and included after that
//!   (tikz's external library, injected from the command line). Needs `--shell-escape`.
//!
//! The timed builds run without the probe and one at a time, since parallel builds slow each
//! other down. Each build gets a warm-up run, then `runs` rounds of the baseline and every
//! variant in turn, so drift hits them all alike. Medians are compared, and a difference no
//! larger than the spread (slowest run minus fastest) is "within noise". One probed run,
//! the cache's when it is current, gives the most each change can save.

use crate::model::{BuildModel, FrameKind};
use crate::run::{self, EngineCmd, RunOptions, RunOutput};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Experiment {
    /// Precompile the preamble into a format (mylatexformat).
    Preamble,
    /// Build without writing a PDF.
    Draft,
    /// Build each tikzpicture once and include it (needs --shell-escape). Experimental.
    Externalize,
}

impl Experiment {
    pub const ALL: [Experiment; 3] = [Experiment::Preamble, Experiment::Draft, Experiment::Externalize];

    pub fn name(self) -> &'static str {
        match self {
            Experiment::Preamble => "preamble",
            Experiment::Draft => "draft",
            Experiment::Externalize => "externalize",
        }
    }

    /// tikz's external library breaks documents in ways a page count and text check do not
    /// always catch.
    pub fn experimental(self) -> bool {
        self == Experiment::Externalize
    }
}

/// Wall times of one build command.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Sample {
    pub median_ms: f64,
    /// Slowest run minus fastest.
    pub spread_ms: f64,
    pub runs_ms: Vec<f64>,
    /// The warm-up run, left out of the median.
    pub warmup_ms: f64,
}

impl Sample {
    fn new(warmup_ms: f64, runs_ms: Vec<f64>) -> Sample {
        Sample { median_ms: round3(median(&runs_ms)), spread_ms: round3(spread(&runs_ms)), runs_ms, warmup_ms }
    }
}

/// Milliseconds to the microsecond, as the other reports print them.
fn round3(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

fn spread(xs: &[f64]) -> f64 {
    let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
    if xs.is_empty() { 0.0 } else { max - min }
}

/// The variant's output against the baseline's.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Output {
    /// Same page count, the same words on every page (`pdftotext`), and the same coarse
    /// render (24 dpi gray; at most 0.5 % of a page's pixels may differ, as in `unused`).
    Identical,
    Changed {
        evidence: String,
    },
    NotCompared {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Measured {
        variant: Sample,
        /// Baseline median minus variant median; negative when the variant is slower.
        saved_ms: f64,
        saved_pct: f64,
        /// The saving is no larger than the larger of the two spreads, or 2 ms (the timing
        /// resolution) when both are smaller.
        within_noise: bool,
        /// Paid once: building the format, or the first externalized run (every picture
        /// built). Not in the median.
        one_time_ms: Option<f64>,
        output: Output,
    },
    /// The variant could not be built: the reason and the first error.
    Failed { error: String },
    /// Not attempted here, and why.
    NotRun { reason: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Row {
    pub experiment: Experiment,
    pub experimental: bool,
    /// The most the probe says the change can save: the preamble phase for `preamble`, the
    /// tikzpicture environments for `externalize`. A cross-check on the measurement.
    pub bound_ms: Option<f64>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WhatifReport {
    pub schema: u32,
    pub job: String,
    pub engine: EngineCmd,
    pub runs: u32,
    pub shell_escape: bool,
    pub baseline: Sample,
    pub baseline_pages: usize,
    pub experiments: Vec<Row>,
}

#[derive(Debug, Clone)]
pub struct WhatifOptions {
    pub main: PathBuf,
    pub engine: EngineCmd,
    /// The experiments to run, in order.
    pub tries: Vec<Experiment>,
    /// `tries` was asked for (`--try`) rather than the default set. An externalize the
    /// user asked for without `--shell-escape` is refused; a default one is not run.
    pub explicit: bool,
    /// Timed runs per build after the warm-up.
    pub runs: u32,
    /// Pass `-shell-escape` to every build. Externalize needs it.
    pub shell_escape: bool,
    /// Per build, in seconds; 0 = 10× the probed build, at least 30 s.
    pub timeout_secs: u64,
    pub out: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum WhatifError {
    #[error(transparent)]
    Run(#[from] run::RunError),
    #[error("the build failed ({}); fix the document first:\n{tail}", run::exit_text(*.code))]
    BaselineFailed { code: Option<i32>, tail: String },
    #[error("pdftotext is not on PATH (poppler-utils); whatif compares each variant's text with the baseline's")]
    NoPdftotext,
    #[error(
        "externalize runs a shell command for every picture (tikz builds each one with its own engine call); pass --shell-escape to allow it"
    )]
    NeedsShellEscape,
    #[error("cannot {what} {path}: {source}")]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The binary that builds a format for `engine`. LuaLaTeX's format is LuaHBTeX's since
/// TeX Live 2020.
fn ini_binary(engine: EngineCmd) -> &'static str {
    match engine {
        EngineCmd::Pdflatex => "pdftex",
        EngineCmd::Xelatex => "xetex",
        EngineCmd::Lualatex if crate::doctor::which("luahbtex").is_some() => "luahbtex",
        EngineCmd::Lualatex => "luatex",
    }
}

/// The switch that stops the engine writing a PDF.
fn draft_flag(engine: EngineCmd) -> &'static str {
    match engine {
        EngineCmd::Pdflatex => "-draftmode",
        EngineCmd::Xelatex => "-no-pdf",
        EngineCmd::Lualatex => "--draftmode",
    }
}

/// The code that turns externalization on, for the command line. The picture builds need
/// it too: tikz runs each one as `\def\tikzexternalrealjob{<job>}\input{<job>}`, which
/// without the hook reads the document with no external library and typesets all of it as
/// "the picture", a whole page put where the picture was. So the system call carries the
/// hook as well, and names the engine the document runs on (tikz's default is pdflatex
/// unless the driver says otherwise).
pub(crate) fn externalize_pretex(engine: EngineCmd) -> String {
    let inner = "\\string\\AddToHook{package/tikz/after}{\\string\\usetikzlibrary{external}\\string\\tikzexternalize}";
    format!(
        "\\AddToHook{{package/tikz/after}}{{\\usetikzlibrary{{external}}\\tikzexternalize[system call={{{} \\tikzexternalcheckshellescape -halt-on-error -interaction=batchmode -jobname \"\\image\" \"{inner}\\texsource\"}}]}}",
        engine.command()
    )
}

/// One build command of an experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Baseline,
    /// Precompile the preamble: `<job>-fmt.fmt`.
    Format,
    /// The document on that format.
    Precompiled,
    Draft,
    Externalize,
}

/// The command line of a step, run in the copy's directory.
pub(crate) fn argv(step: Step, engine: EngineCmd, job: &str, shell_escape: bool) -> Vec<String> {
    let mut v = vec![engine.command().to_string(), "-interaction=nonstopmode".to_string()];
    if shell_escape || step == Step::Externalize {
        v.push("-shell-escape".into());
    }
    let tex = format!("{job}.tex");
    match step {
        Step::Baseline => v.push(tex),
        Step::Format => {
            v[0] = ini_binary(engine).to_string();
            v.insert(1, "-ini".into());
            v.extend([
                format!("-jobname={job}-fmt"),
                format!("&{}", engine.command()),
                "mylatexformat.ltx".into(),
                tex,
            ]);
        }
        Step::Precompiled => v.extend([format!("-fmt={job}-fmt"), tex]),
        Step::Draft => v.extend([draft_flag(engine).to_string(), tex]),
        Step::Externalize => {
            v.extend([format!("-jobname={job}"), format!("{}\\input{{{job}}}", externalize_pretex(engine))])
        }
    }
    v
}

/// The first thing that went wrong in a failed build: its first `!` error, else a Lua error
/// (LuaTeX reports those without `!`), else the exit status.
pub(crate) fn first_error(log_text: &str, code: Option<i32>) -> String {
    let log = crate::texlog::parse_str(log_text);
    if let Some(e) = log.errors.first() {
        return format!("! {e}");
    }
    // LuaTeX wraps at max_print_line or one past it, which `texlog::unwrap` leaves alone.
    let lines: Vec<&str> = log_text.lines().collect();
    for (k, line) in lines.iter().enumerate() {
        if lua_error(line).is_none() {
            continue;
        }
        let mut joined = line.to_string();
        let mut j = k;
        while lines[j].chars().count() >= crate::texlog::MAX_PRINT_LINE
            && lines.get(j + 1).is_some_and(|n| !n.is_empty() && !n.starts_with('<') && !n.starts_with('!'))
        {
            j += 1;
            joined.push_str(lines[j]);
        }
        return lua_error(&joined).unwrap_or(joined);
    }
    run::exit_text(code)
}

/// `.../lua-uni-stage-tables.lua:141: attempt to ...` as `lua-uni-stage-tables.lua:141:
/// attempt to ...`.
fn lua_error(line: &str) -> Option<String> {
    let i = line.find(".lua:")?;
    let rest = &line[i + ".lua:".len()..];
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || !rest[digits..].starts_with(": ") {
        return None;
    }
    let start = line[..i].rfind(['/', ' ', '(', ')']).map_or(0, |j| j + 1);
    Some(line[start..].trim().to_string())
}

/// What a failed build is called in the row.
fn failure(step: Step, error: &str) -> String {
    match step {
        Step::Format => format!("could not precompile: {error}"),
        Step::Precompiled => format!("the build on the precompiled preamble failed: {error}"),
        Step::Draft => format!("the draft build failed: {error}"),
        Step::Externalize => format!("could not externalize: {error}"),
        Step::Baseline => error.to_string(),
    }
}

/// Run one step in `dir`: its wall time in ms, or the first error.
fn build(
    dir: &Path,
    step: Step,
    opts: &WhatifOptions,
    job: &str,
    timeout: std::time::Duration,
) -> Result<f64, (Option<i32>, String)> {
    let args = argv(step, opts.engine, job, opts.shell_escape);
    let log_job = if step == Step::Format { format!("{job}-fmt") } else { job.to_string() };
    let out = dir.join(format!("prumo-{}.txt", step_name(step)));
    match run::spawn_timed(dir, &args, &opts.env, &out, Some(timeout)) {
        Ok(b) if b.exit_code == Some(0) => Ok(round3(b.wall.as_secs_f64() * 1000.0)),
        Ok(b) => {
            let text = std::fs::read(dir.join(format!("{log_job}.log")))
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default();
            Err((b.exit_code, first_error(&text, b.exit_code)))
        }
        Err(run::RunError::TimedOut { secs }) => Err((None, format!("timed out after {secs:.0} s"))),
        Err(e) => Err((None, e.to_string())),
    }
}

fn step_name(step: Step) -> &'static str {
    match step {
        Step::Baseline => "baseline",
        Step::Format => "format",
        Step::Precompiled => "preamble",
        Step::Draft => "draft",
        Step::Externalize => "externalize",
    }
}

/// The probed run, from the cache when it is current: the bounds come from it, and a
/// document that does not build stops here.
fn probed_run(opts: &WhatifOptions, out_root: &Path) -> Result<RunOutput, WhatifError> {
    let ro = RunOptions {
        build: opts.shell_escape.then(|| format!("{} -shell-escape", opts.engine.command())),
        out: Some(out_root.to_path_buf()),
        env: opts.env.clone(),
        ..RunOptions::new(opts.main.clone(), opts.engine)
    };
    let out = match run::cached(&opts.main, Some(out_root))?.filter(|c| !run::is_stale(c, &ro)) {
        Some(c) => c,
        None => run::run(&ro)?,
    };
    if out.info.exit_code != Some(0) {
        let text = std::fs::read(&out.log).map(|t| String::from_utf8_lossy(&t).into_owned()).unwrap_or_default();
        return Err(WhatifError::BaselineFailed {
            code: out.info.exit_code,
            tail: first_error(&text, out.info.exit_code),
        });
    }
    Ok(out)
}

/// Some package in the load tree has this name.
fn loads(model: &BuildModel, package: &str) -> bool {
    let mut found = false;
    model.root.walk(&mut |f, _| found |= f.kind == FrameKind::Pkg && f.name == package);
    found
}

/// The probe's bound for an experiment, in ms.
pub fn bound_ms(model: &BuildModel, e: Experiment) -> Option<f64> {
    match e {
        Experiment::Preamble => model.run_phases().preamble.map(crate::report::ms),
        Experiment::Externalize => Some(crate::report::ms(
            model.env_table().iter().find(|r| r.name == "tikzpicture").map(|r| r.total).unwrap_or(0),
        )),
        Experiment::Draft => None,
    }
}

/// Why an experiment cannot run on this document here, if it cannot.
fn not_run_reason(e: Experiment, model: &BuildModel, opts: &WhatifOptions) -> Option<String> {
    match e {
        Experiment::Preamble if crate::doctor::tex_file("mylatexformat.ltx").is_none() => Some(format!(
            "mylatexformat is not installed ({})",
            crate::doctor::Installer::detect().install("mylatexformat")
        )),
        Experiment::Externalize if !loads(model, "tikz") => Some("the document does not load tikz".into()),
        Experiment::Externalize if model.root.find("tikzlibraryexternal.code.tex").is_some() => {
            Some("the document already externalizes its pictures".into())
        }
        Experiment::Externalize if !opts.shell_escape => {
            Some("needs --shell-escape (tikz builds each picture with a shell command)".into())
        }
        _ => None,
    }
}

/// What the output comparison looks at: page count, each page's words, a coarse render.
pub(crate) struct PdfLook {
    pages: usize,
    words: Vec<Vec<String>>,
    render: Option<Vec<Vec<u8>>>,
}

/// The dpi and tolerance of the render check: `unused`'s coarse ink check.
const RENDER_DPI: u32 = 24;
const RENDER_PER_MILLE: usize = 5;

fn look_at(pdf: &Path) -> Option<PdfLook> {
    let text = crate::unused::pdftotext(pdf)?;
    let pages = crate::unused::pdfinfo(pdf).and_then(|i| i.get("Pages").and_then(|p| p.parse().ok())).unwrap_or(0);
    Some(PdfLook { pages, words: page_words(&text), render: crate::unused::render(pdf, RENDER_DPI) })
}

/// Each page's words, in reading order (pages are split at form feeds). Words rather than
/// lines: an externalized picture is an included PDF, and `pdftotext -layout` can place its
/// text a column or a line away from where it put the same text drawn inline.
pub(crate) fn page_words(text: &str) -> Vec<Vec<String>> {
    let mut pages: Vec<Vec<String>> =
        text.split('\u{c}').map(|p| p.split_whitespace().map(str::to_string).collect()).collect();
    while pages.last().is_some_and(|p| p.is_empty()) {
        pages.pop();
    }
    pages
}

/// Where two pages' words differ, sorted, so order within a page does not count: the first
/// word one page has more of than the other.
pub(crate) fn words_differ(a: &[String], b: &[String]) -> Option<String> {
    fn count(ws: &[String]) -> std::collections::BTreeMap<&str, i64> {
        let mut m = std::collections::BTreeMap::new();
        for w in ws {
            *m.entry(w.as_str()).or_default() += 1;
        }
        m
    }
    let (ca, cb) = (count(a), count(b));
    let keys: std::collections::BTreeSet<&str> = ca.keys().chain(cb.keys()).copied().collect();
    keys.into_iter().find_map(|k| {
        let (x, y) = (ca.get(k).copied().unwrap_or(0), cb.get(k).copied().unwrap_or(0));
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(format!("{k:?} only in the baseline")),
            std::cmp::Ordering::Less => Some(format!("{k:?} only in the variant")),
        }
    })
}

fn compare_output(base: &PdfLook, pdf: &Path) -> Output {
    let Some(v) = look_at(pdf) else {
        return Output::Changed { evidence: "no readable PDF".into() };
    };
    if v.pages != base.pages {
        return Output::Changed {
            evidence: format!("{} page{} vs {}", v.pages, if v.pages == 1 { "" } else { "s" }, base.pages),
        };
    }
    for (i, (a, b)) in base.words.iter().zip(&v.words).enumerate() {
        if let Some(w) = words_differ(a, b) {
            return Output::Changed { evidence: format!("page {}: {w}", i + 1) };
        }
    }
    if base.words.len() != v.words.len() {
        return Output::Changed { evidence: "the text has a different number of pages".into() };
    }
    if let (Some(a), Some(b)) = (&base.render, &v.render)
        && let Some((page, differing, total)) = crate::unused::render_diff(a, b, RENDER_PER_MILLE)
    {
        return Output::Changed {
            evidence: format!(
                "page {page} looks different ({differing} of {total} pixels at {RENDER_DPI} dpi; same words)"
            ),
        };
    }
    Output::Identical
}

/// One experiment's copy of the project and what it measured so far.
struct Lane {
    experiment: Experiment,
    step: Step,
    dir: PathBuf,
    warmup_ms: f64,
    one_time_ms: Option<f64>,
    runs_ms: Vec<f64>,
    failed: Option<String>,
}

/// Save a build directory's logs and engine output to the cache, so a failure can be read
/// after the temp copy is gone.
fn keep_logs(dir: &Path, to: &Path) {
    if std::fs::create_dir_all(to).is_err() {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("prumo-") && name.ends_with(".txt") || name.ends_with(".log") {
            let _ = std::fs::copy(e.path(), to.join(&name));
        }
    }
}

/// The whole pass. `progress` gets one line per step.
pub fn run(opts: &WhatifOptions, mut progress: impl FnMut(&str)) -> Result<WhatifReport, WhatifError> {
    if crate::doctor::which("pdftotext").is_none() {
        return Err(WhatifError::NoPdftotext);
    }
    if opts.explicit && opts.tries.contains(&Experiment::Externalize) && !opts.shell_escape {
        return Err(WhatifError::NeedsShellEscape);
    }
    let (job, project) = run::job_of(&opts.main)?;
    let out_root = match &opts.out {
        Some(o) => o.clone(),
        None => run::default_out(&opts.main)?,
    };
    let logs = out_root.join(&job).join("whatif");
    progress(&format!("probed run and logs in {}", crate::doctor::host_path(&out_root.join(&job))));
    let probed = probed_run(opts, &out_root)?;
    let model = run::model_of(&probed)?;
    let probed_ms = crate::report::ms(model.wall.unwrap_or(model.end));
    let timeout = std::time::Duration::from_secs_f64(if opts.timeout_secs > 0 {
        opts.timeout_secs as f64
    } else {
        (probed_ms / 1000.0 * 10.0).max(30.0)
    });
    // Every picture is a build of its own on the first externalized run.
    let pictures = model.env_table().iter().find(|r| r.name == "tikzpicture").map(|r| r.count).unwrap_or(0);

    let tmp = tempfile::Builder::new().prefix("prumo-whatif-").tempdir().map_err(|source| WhatifError::Io {
        what: "create",
        path: std::env::temp_dir(),
        source,
    })?;
    let stage = |name: &str| -> Result<PathBuf, WhatifError> {
        let d = tmp.path().join(name);
        run::copy_tree(&project, &d)?;
        Ok(d)
    };
    let mut rows: Vec<(Experiment, Option<f64>, Option<Outcome>)> = Vec::new();
    let mut lanes: Vec<Lane> = Vec::new();
    let mut tries: Vec<Experiment> = Vec::new();
    for e in &opts.tries {
        if !tries.contains(e) {
            tries.push(*e);
        }
    }
    for e in tries {
        let bound = bound_ms(&model, e);
        match not_run_reason(e, &model, opts) {
            Some(reason) => rows.push((e, bound, Some(Outcome::NotRun { reason }))),
            None => {
                rows.push((e, bound, None));
                let step = match e {
                    Experiment::Preamble => Step::Precompiled,
                    Experiment::Draft => Step::Draft,
                    Experiment::Externalize => Step::Externalize,
                };
                lanes.push(Lane {
                    experiment: e,
                    step,
                    dir: stage(e.name())?,
                    warmup_ms: 0.0,
                    one_time_ms: None,
                    runs_ms: vec![],
                    failed: None,
                });
            }
        }
    }
    let base_dir = stage("baseline")?;
    let baseline_failed = |(code, e): (Option<i32>, String)| {
        keep_logs(&base_dir, &logs.join("baseline"));
        WhatifError::BaselineFailed { code, tail: e }
    };

    // Warm-up: the baseline, then each variant, the format first. A variant that fails
    // here is out; its row says why.
    let runs = opts.runs.max(1);
    progress(&format!(
        "{} on {}: a warm-up, then {runs} timed run{} of the baseline and {} variant{}, one build at a time",
        job,
        opts.engine.command(),
        if runs == 1 { "" } else { "s" },
        lanes.len(),
        if lanes.len() == 1 { "" } else { "s" }
    ));
    let base_warm = build(&base_dir, Step::Baseline, opts, &job, timeout).map_err(baseline_failed)?;
    for lane in &mut lanes {
        if lane.experiment == Experiment::Preamble {
            progress("preamble: building the format");
            match build(&lane.dir, Step::Format, opts, &job, timeout) {
                Ok(ms) => lane.one_time_ms = Some(ms),
                Err((_, e)) => {
                    lane.failed = Some(failure(Step::Format, &e));
                    continue;
                }
            }
        }
        progress(&format!("{}: warm-up", lane.experiment.name()));
        let t = if lane.step == Step::Externalize { timeout * (1 + pictures) } else { timeout };
        match build(&lane.dir, lane.step, opts, &job, t) {
            Ok(ms) => {
                lane.warmup_ms = ms;
                if lane.step == Step::Externalize {
                    lane.one_time_ms = Some(ms);
                }
            }
            Err((_, e)) => lane.failed = Some(failure(lane.step, &e)),
        }
    }
    let mut base_runs = Vec::new();
    for r in 1..=runs {
        progress(&format!("run {r}/{runs}"));
        base_runs.push(build(&base_dir, Step::Baseline, opts, &job, timeout).map_err(baseline_failed)?);
        for lane in lanes.iter_mut().filter(|l| l.failed.is_none()) {
            match build(&lane.dir, lane.step, opts, &job, timeout) {
                Ok(ms) => lane.runs_ms.push(ms),
                Err((_, e)) => lane.failed = Some(failure(lane.step, &e)),
            }
        }
    }
    let baseline = Sample::new(base_warm, base_runs);
    let base_pdf = base_dir.join(format!("{job}.pdf"));
    let base_out = look_at(&base_pdf)
        .ok_or_else(|| WhatifError::BaselineFailed { code: Some(0), tail: format!("no readable {job}.pdf") })?;
    keep_logs(&base_dir, &logs.join("baseline"));

    let mut experiments = Vec::new();
    for (e, bound, outcome) in rows {
        let outcome = match outcome {
            Some(o) => o,
            None => {
                let lane = lanes.iter().find(|l| l.experiment == e).expect("a lane per row that ran");
                keep_logs(&lane.dir, &logs.join(e.name()));
                match &lane.failed {
                    Some(error) => Outcome::Failed { error: error.clone() },
                    None => measured(&baseline, lane, &base_out, &job),
                }
            }
        };
        experiments.push(Row { experiment: e, experimental: e.experimental(), bound_ms: bound, outcome });
    }
    Ok(WhatifReport {
        schema: crate::JSON_SCHEMA,
        job,
        engine: opts.engine,
        runs,
        shell_escape: opts.shell_escape,
        baseline,
        baseline_pages: base_out.pages,
        experiments,
    })
}

fn measured(baseline: &Sample, lane: &Lane, base_out: &PdfLook, job: &str) -> Outcome {
    let variant = Sample::new(lane.warmup_ms, lane.runs_ms.clone());
    let output = match lane.step {
        Step::Draft => Output::NotCompared { reason: "draft mode writes no PDF".into() },
        _ => compare_output(base_out, &lane.dir.join(format!("{job}.pdf"))),
    };
    outcome_of(baseline, variant, lane.one_time_ms, output)
}

/// The smallest difference that is not noise: wall times are polled every millisecond
/// (`run::spawn_timed`), so each run can read up to 1 ms late and two runs can differ by 2 ms
/// on resolution alone.
const MIN_NOISE_MS: f64 = 2.0;

/// The numbers of a measured row.
pub(crate) fn outcome_of(baseline: &Sample, variant: Sample, one_time_ms: Option<f64>, output: Output) -> Outcome {
    let saved_ms = round3(baseline.median_ms - variant.median_ms);
    let saved_pct =
        if baseline.median_ms > 0.0 { (saved_ms / baseline.median_ms * 1000.0).round() / 10.0 } else { 0.0 };
    let within_noise = saved_ms.abs() <= baseline.spread_ms.max(variant.spread_ms).max(MIN_NOISE_MS);
    Outcome::Measured { variant, saved_ms, saved_pct, within_noise, one_time_ms, output }
}

/// How to make the change for real, for the notes under the table.
fn how(e: Experiment, engine: EngineCmd, job: &str) -> String {
    match e {
        Experiment::Preamble => format!(
            "once per preamble change, {} -ini -jobname={job}-fmt \"&{}\" mylatexformat.ltx {job}.tex; then {} -fmt={job}-fmt {job}.tex",
            ini_binary(engine),
            engine.command(),
            engine.command()
        ),
        Experiment::Draft => format!(
            "{} {} {job}.tex while writing, and a full build before reading the PDF. The saving is what writing the PDF and including images cost",
            engine.command(),
            draft_flag(engine)
        ),
        Experiment::Externalize => format!(
            "\\usetikzlibrary{{external}}\\tikzexternalize in the preamble and {} -shell-escape. A picture is rebuilt only when it changes",
            engine.command()
        ),
    }
}

const EXPERIMENTAL: &str = "  * experimental: tikz's external library breaks remember picture, overlays and references between pictures. Check the PDF, not only this table.";

pub fn text(r: &WhatifReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "== prumo whatif: {} ({}, median of {} run{} after a warm-up, {} page{}{})",
        r.job,
        r.engine.command(),
        r.runs,
        if r.runs == 1 { "" } else { "s" },
        r.baseline_pages,
        if r.baseline_pages == 1 { "" } else { "s" },
        if r.shell_escape { ", -shell-escape" } else { "" }
    );
    // The experimental mark goes on rows that ran; a row that did not run has nothing to doubt.
    let ran = |row: &Row| !matches!(row.outcome, Outcome::NotRun { .. });
    let label = |row: &Row| format!("{}{}", row.experiment.name(), if row.experimental && ran(row) { "*" } else { "" });
    let _ = writeln!(
        s,
        "  {:<13} {:>9} {:>9}  {:<14} {:>8} {:>9}  output",
        "experiment", "baseline", "variant", "saved", "bound", "one-time"
    );
    let ms0 = |v: f64| format!("{v:.0} ms");
    for row in &r.experiments {
        let bound = row.bound_ms.map(ms0).unwrap_or_else(|| "-".into());
        match &row.outcome {
            Outcome::Measured { variant, saved_ms, saved_pct, within_noise, one_time_ms, output } => {
                let saved = if *within_noise {
                    "within noise".to_string()
                } else if *saved_ms >= 0.0 {
                    format!("{saved_ms:.0} ms {saved_pct:.0}%")
                } else {
                    format!("{:.0} ms slower", -saved_ms)
                };
                let output = match output {
                    Output::Identical => "identical".to_string(),
                    Output::Changed { evidence } => format!("CHANGED ({evidence})"),
                    Output::NotCompared { .. } => "n/a (no PDF)".to_string(),
                };
                let _ = writeln!(
                    s,
                    "  {:<13} {:>9} {:>9}  {:<14} {:>8} {:>9}  {output}",
                    label(row),
                    ms0(r.baseline.median_ms),
                    ms0(variant.median_ms),
                    saved,
                    bound,
                    one_time_ms.map(ms0).unwrap_or_else(|| "-".into()),
                );
            }
            Outcome::Failed { error } => {
                let _ = writeln!(s, "  {:<13} {error}", label(row));
            }
            Outcome::NotRun { reason } => {
                let _ = writeln!(s, "  {:<13} not run: {reason}", label(row));
            }
        }
    }
    let measured: Vec<(&Row, &Sample)> = r
        .experiments
        .iter()
        .filter_map(|row| match &row.outcome {
            Outcome::Measured { variant, .. } => Some((row, variant)),
            _ => None,
        })
        .collect();
    let experimental = r.experiments.iter().any(|row| row.experimental && ran(row));
    if measured.is_empty() && !experimental {
        return s;
    }
    s.push('\n');
    if measured.is_empty() {
        let _ = writeln!(s, "{EXPERIMENTAL}");
        return s;
    }
    let spreads: Vec<String> = std::iter::once(format!("baseline {:.0} ms", r.baseline.spread_ms))
        .chain(measured.iter().map(|(row, v)| format!("{} {:.0} ms", row.experiment.name(), v.spread_ms)))
        .collect();
    let _ = writeln!(
        s,
        "  spread (slowest run minus fastest): {}. A difference no larger than the larger spread (2 ms at least) is noise.",
        spreads.join(", ")
    );
    if measured.iter().any(|(row, _)| row.bound_ms.is_some()) {
        let _ = writeln!(
            s,
            "  bound: the most the probe says the change can save (the preamble phase; the tikzpicture environments)."
        );
    }
    for (row, _) in &measured {
        let Outcome::Measured { saved_ms, within_noise, output, .. } = &row.outcome else { continue };
        let name = row.experiment.name();
        if let Output::Changed { evidence } = output
            && !within_noise
        {
            let _ =
                writeln!(s, "  {name}: the output changed ({evidence}), so {saved_ms:.0} ms is not a clean saving.");
        }
        if let Some(b) = row.bound_ms
            && !within_noise
            && *saved_ms > b + r.baseline.spread_ms
        {
            let _ = writeln!(
                s,
                "  {name}: saved {saved_ms:.0} ms, more than the probe's bound of {b:.0} ms. One of the two is off; run again with more --runs."
            );
        }
    }
    if experimental {
        let _ = writeln!(s, "{EXPERIMENTAL}");
    }
    s.push('\n');
    for (row, _) in &measured {
        let _ = writeln!(s, "  {}: {}", row.experiment.name(), how(row.experiment, r.engine, &r.job));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_compared_by_the_words_on_each_page() {
        let base = "A  title\n  x = 1 (1)\n\u{c}Table 1\n  a   b\n\u{c}";
        // The same words, one moved a line down and the table a column right.
        let moved = "A title\n\n  (1) x = 1\n\u{c} Table 1\n   a   b\n\u{c}";
        let (a, b) = (page_words(base), page_words(moved));
        assert_eq!(a.len(), 2, "the trailing form feed is not a page");
        assert!(a.iter().zip(&b).all(|(x, y)| words_differ(x, y).is_none()));
        let changed = page_words("A title\n x = 2 (1)\n\u{c}Table 1\n a b\n");
        assert_eq!(words_differ(&a[0], &changed[0]).as_deref(), Some("\"1\" only in the baseline"));
    }

    #[test]
    fn median_and_spread() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(spread(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!((median(&[]), spread(&[])), (0.0, 0.0));
    }

    #[test]
    fn noise_is_the_larger_spread() {
        let base = Sample::new(900.0, vec![880.0, 870.0, 890.0]);
        assert_eq!((base.median_ms, base.spread_ms), (880.0, 20.0));
        let Outcome::Measured { saved_ms, saved_pct, within_noise, .. } =
            outcome_of(&base, Sample::new(600.0, vec![564.0, 560.0, 570.0]), Some(622.0), Output::Identical)
        else {
            panic!()
        };
        assert_eq!((saved_ms, within_noise), (316.0, false));
        assert_eq!(saved_pct, 35.9);
        let Outcome::Measured { within_noise, .. } =
            outcome_of(&base, Sample::new(0.0, vec![847.0, 820.0, 870.0]), None, Output::Identical)
        else {
            panic!()
        };
        assert!(within_noise, "33 ms saved, the variant's spread is 50 ms");
        let Outcome::Measured { saved_ms, within_noise, .. } =
            outcome_of(&base, Sample::new(0.0, vec![950.0, 951.0, 952.0]), None, Output::Identical)
        else {
            panic!()
        };
        assert_eq!((saved_ms, within_noise), (-71.0, false), "slower is a result too");
        let quiet = Sample::new(0.0, vec![137.0, 137.0, 137.0]);
        let Outcome::Measured { within_noise, .. } =
            outcome_of(&quiet, Sample::new(0.0, vec![136.0, 136.0, 136.0]), None, Output::Identical)
        else {
            panic!()
        };
        assert!(within_noise, "1 ms is below the 1 ms polling of two runs");
    }

    #[test]
    fn command_lines() {
        let a = |s, e, se| argv(s, e, "main", se);
        assert_eq!(a(Step::Baseline, EngineCmd::Pdflatex, false), ["pdflatex", "-interaction=nonstopmode", "main.tex"]);
        assert_eq!(
            a(Step::Format, EngineCmd::Pdflatex, false),
            [
                "pdftex",
                "-ini",
                "-interaction=nonstopmode",
                "-jobname=main-fmt",
                "&pdflatex",
                "mylatexformat.ltx",
                "main.tex"
            ]
        );
        assert_eq!(
            a(Step::Precompiled, EngineCmd::Xelatex, true),
            ["xelatex", "-interaction=nonstopmode", "-shell-escape", "-fmt=main-fmt", "main.tex"]
        );
        assert_eq!(a(Step::Draft, EngineCmd::Pdflatex, false)[2], "-draftmode");
        assert_eq!(a(Step::Draft, EngineCmd::Xelatex, false)[2], "-no-pdf");
        assert_eq!(a(Step::Draft, EngineCmd::Lualatex, false)[2], "--draftmode");
        let x = a(Step::Externalize, EngineCmd::Lualatex, false);
        assert_eq!(&x[..4], ["lualatex", "-interaction=nonstopmode", "-shell-escape", "-jobname=main"]);
        assert_eq!(
            x[4],
            "\\AddToHook{package/tikz/after}{\\usetikzlibrary{external}\\tikzexternalize[system call={lualatex \\tikzexternalcheckshellescape -halt-on-error -interaction=batchmode -jobname \"\\image\" \"\\string\\AddToHook{package/tikz/after}{\\string\\usetikzlibrary{external}\\string\\tikzexternalize}\\texsource\"}]}\\input{main}"
        );
        assert_eq!(x[4].matches('{').count(), x[4].matches('}').count());
    }

    #[test]
    fn first_error_of_a_failed_build() {
        let xetex = "entering extended mode\n! Can't \\dump a format with native fonts or font-mappings.\n<*> &xelatex mylatexformat.ltx main.tex\n";
        assert_eq!(first_error(xetex, Some(1)), "! Can't \\dump a format with native fonts or font-mappings.");
        // LuaTeX's dump on TeX Live 2026: a Lua error, wrapped at 79 columns, and no `!`.
        let luatex = "(/usr/local/texlive/2026/texmf-dist/tex/latex/lm/t1lmss.fd\nFile: t1lmss.fd 2015/05/01 v1.6.1 Font defs for Latin Modern\n))...f-dist/tex/luatex/lua-uni-algos/lua-uni-stage-tables.lua:141: attempt to pe\nrform arithmetic on a string value (local 'key').\n<*> &lualatex mylatexformat.ltx main.tex\n";
        assert_eq!(
            first_error(luatex, Some(1)),
            "lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key')."
        );
        assert_eq!(first_error("nothing to see\n", Some(3)), "exit 3");
        assert_eq!(lua_error("see foo.lua:x: no"), None);
    }

    fn report(rows: Vec<Row>) -> WhatifReport {
        WhatifReport {
            schema: 1,
            job: "main".into(),
            engine: EngineCmd::Pdflatex,
            runs: 5,
            shell_escape: true,
            baseline: Sample::new(900.0, vec![880.0, 875.0, 885.0, 878.0, 890.0]),
            baseline_pages: 2,
            experiments: rows,
        }
    }

    fn row(e: Experiment, bound: Option<f64>, outcome: Outcome) -> Row {
        Row { experiment: e, experimental: e.experimental(), bound_ms: bound, outcome }
    }

    /// The shape of the brief's example: a clean saving, one within noise, one whose output
    /// changed.
    #[test]
    fn table_says_what_is_noise_and_what_changed() {
        let r = report(vec![]);
        let b = r.baseline.clone();
        let r = report(vec![
            row(
                Experiment::Preamble,
                Some(372.0),
                outcome_of(
                    &b,
                    Sample::new(0.0, vec![564.0, 560.0, 566.0, 563.0, 570.0]),
                    Some(622.0),
                    Output::Identical,
                ),
            ),
            row(
                Experiment::Draft,
                None,
                outcome_of(
                    &b,
                    Sample::new(0.0, vec![847.0, 830.0, 870.0, 850.0, 845.0]),
                    None,
                    Output::NotCompared { reason: "draft mode writes no PDF".into() },
                ),
            ),
            row(
                Experiment::Externalize,
                Some(300.0),
                outcome_of(
                    &b,
                    Sample::new(1303.0, vec![464.0, 460.0, 470.0, 466.0, 462.0]),
                    Some(1303.0),
                    Output::Changed { evidence: "3 pages vs 2".into() },
                ),
            ),
        ]);
        let t = text(&r);
        assert!(t.contains("preamble         880 ms    564 ms  316 ms 36%       372 ms    622 ms  identical"), "{t}");
        assert!(
            t.contains("draft            880 ms    847 ms  within noise          -         -  n/a (no PDF)"),
            "{t}"
        );
        assert!(t.contains("externalize*     880 ms    464 ms  416 ms 47%"), "{t}");
        assert!(t.contains("CHANGED (3 pages vs 2)"), "{t}");
        assert!(t.contains("externalize: the output changed (3 pages vs 2), so 416 ms is not a clean saving"), "{t}");
        assert!(t.contains("externalize: saved 416 ms, more than the probe's bound of 300 ms"), "{t}");
        assert!(!t.contains("preamble: saved"), "316 ms is inside the bound of 372: {t}");
        assert!(t.contains("* experimental"), "{t}");
        assert!(t.contains("spread (slowest run minus fastest): baseline 15 ms, preamble 10 ms, draft 40 ms"), "{t}");
        assert!(t.contains("pdftex -ini -jobname=main-fmt \"&pdflatex\" mylatexformat.ltx main.tex"), "{t}");
        assert!(t.contains("draft: pdflatex -draftmode main.tex"), "{t}");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["experiments"][0]["status"], "measured");
        assert_eq!(json["experiments"][0]["output"]["status"], "identical");
        assert_eq!(json["experiments"][1]["within_noise"], true);
        assert_eq!(json["experiments"][2]["experimental"], true);
        assert_eq!(json["experiments"][2]["output"]["evidence"], "3 pages vs 2");
    }

    #[test]
    fn failures_and_skips_are_rows() {
        let r = report(vec![
            row(
                Experiment::Preamble,
                Some(300.0),
                Outcome::Failed {
                    error: "could not precompile: ! Can't \\dump a format with native fonts or font-mappings.".into(),
                },
            ),
            row(
                Experiment::Externalize,
                Some(40.0),
                Outcome::NotRun { reason: "the document does not load tikz".into() },
            ),
        ]);
        let t = text(&r);
        assert!(t.contains("preamble      could not precompile: ! Can't \\dump a format"), "{t}");
        assert!(t.contains("externalize   not run: the document does not load tikz"), "{t}");
        assert!(!t.contains("spread") && !t.contains("experimental"), "nothing ran, no notes: {t}");
        let failed = report(vec![row(
            Experiment::Externalize,
            Some(40.0),
            Outcome::Failed { error: "could not externalize: ! Package tikz Error".into() },
        )]);
        let t = text(&failed);
        assert!(t.contains("externalize*  could not externalize") && t.contains("* experimental"), "{t}");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["experiments"][0]["status"], "failed");
        assert_eq!(json["experiments"][1]["status"], "not-run");
    }
}
