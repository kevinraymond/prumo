//! `prumo compare`: the same document on every engine this machine has, one table.
//!
//! The requester's question was "maybe I could find a compiler that is two seconds faster";
//! the honest answer is specific to his documents, so this runs them. Each engine gets its
//! own cache (`compare/<engine>/<job>/` under the document's cache directory). The first run on XeTeX and LuaTeX pays for
//! font caches (findings 20 and 26), so by default every engine runs twice and the second
//! run is reported; the first is shown alongside when it differs.

use crate::Scaled;
use crate::model::BuildModel;
use crate::report::ms;
use crate::run::{self, EngineCmd, RunOptions};
use std::path::PathBuf;

pub const ALL_ENGINES: [EngineCmd; 3] = [EngineCmd::Pdflatex, EngineCmd::Xelatex, EngineCmd::Lualatex];

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Ok {
        total_ms: f64,
        preamble_ms: f64,
        body_ms: f64,
        pages: usize,
        /// Slowest package or class by exclusive time.
        slowest_unit: Option<String>,
        slowest_unit_ms: f64,
        page1_ms: f64,
        /// Mean of pages after the first, when there are any.
        later_pages_ms: Option<f64>,
        /// Total of the discarded warm-up run, when `runs > 1`.
        first_run_ms: Option<f64>,
    },
    NotInstalled,
    Failed {
        exit_code: Option<i32>,
        log_tail: String,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Row {
    pub engine: EngineCmd,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Comparison {
    pub schema: u32,
    pub job: String,
    pub runs: u32,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone)]
pub struct CompareOptions {
    pub main: PathBuf,
    pub engines: Vec<EngineCmd>,
    pub latexmk: bool,
    /// Runs per engine; the last one is reported.
    pub runs: u32,
    pub out: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

fn total_of(model: &BuildModel) -> Scaled {
    model.total()
}

/// Run and measure. `progress` is called before each build with a one-line message.
pub fn compare(opts: &CompareOptions, mut progress: impl FnMut(&str)) -> Result<Comparison, run::RunError> {
    let (job, _) = run::job_of(&opts.main)?;
    let base_out = match &opts.out {
        Some(o) => o.join("compare"),
        None => run::default_out(&opts.main)?.join("compare"),
    };
    progress(&format!("artifacts in {}", base_out.display()));
    let mut rows = Vec::new();
    for &engine in &opts.engines {
        let tool = if opts.latexmk { "latexmk" } else { engine.command() };
        if crate::doctor::which(tool).is_none() || crate::doctor::which(engine.command()).is_none() {
            rows.push(Row { engine, outcome: Outcome::NotInstalled });
            continue;
        }
        let out_dir = base_out.join(engine.command());
        let mut first_total: Option<Scaled> = None;
        let mut last: Option<(run::RunOutput, BuildModel)> = None;
        let mut failed: Option<Outcome> = None;
        for i in 0..opts.runs.max(1) {
            progress(&format!("{}: run {}/{}", engine.command(), i + 1, opts.runs.max(1)));
            let out = run::run(&RunOptions {
                latexmk: opts.latexmk,
                out: Some(out_dir.clone()),
                env: opts.env.clone(),
                ..RunOptions::new(opts.main.clone(), engine)
            })?;
            if out.info.exit_code != Some(0) {
                // The first `! ` error says why; the log tail usually shows an "Output written" line of a build that failed earlier.
                let log = crate::texlog::parse_file(&out.log).ok();
                let evidence = match log.as_ref().and_then(|l| l.errors.first()) {
                    Some(e) => format!("! {e}"),
                    None => std::fs::read_to_string(&out.log).map(|t| run::tail_lines(&t, 4)).unwrap_or_default(),
                };
                failed = Some(Outcome::Failed { exit_code: out.info.exit_code, log_tail: evidence });
                break;
            }
            let model = run::model_of(&out)?;
            if i == 0 {
                first_total = Some(total_of(&model));
            }
            last = Some((out, model));
        }
        if let Some(f) = failed {
            rows.push(Row { engine, outcome: f });
            continue;
        }
        let Some((_, model)) = last else { continue };
        let pages = model.page_times();
        let slowest = model.unit_table().into_iter().find(|r| r.kind.is_unit());
        rows.push(Row {
            engine,
            outcome: Outcome::Ok {
                total_ms: ms(model.total()),
                preamble_ms: ms(model.preamble()),
                body_ms: ms(model.total().saturating_sub(model.preamble())),
                pages: pages.len(),
                slowest_unit: slowest.as_ref().map(|r| r.name.clone()),
                slowest_unit_ms: slowest.as_ref().map(|r| ms(r.exclusive)).unwrap_or(0.0),
                page1_ms: pages.first().map(|p| ms(p.time)).unwrap_or(0.0),
                later_pages_ms: (pages.len() > 1)
                    .then(|| ms(pages[1..].iter().map(|p| p.time).sum::<Scaled>() / (pages.len() as Scaled - 1))),
                first_run_ms: first_total.filter(|_| opts.runs > 1).map(ms),
            },
        });
    }
    Ok(Comparison { schema: crate::JSON_SCHEMA, job, runs: opts.runs.max(1), rows })
}

pub fn text(c: &Comparison) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "== prumo compare: {}  ({} run{} per engine{})",
        c.job,
        c.runs,
        if c.runs == 1 { "" } else { "s" },
        if c.runs > 1 { ", last one reported" } else { "" }
    );
    let _ = writeln!(
        s,
        "  {:<9} {:>8} {:>9} {:>8} {:>5}  {:<28} {:>8} {:>10}",
        "engine", "total", "preamble", "body", "pages", "slowest unit (excl)", "page 1", "first run"
    );
    for r in &c.rows {
        match &r.outcome {
            Outcome::Ok {
                total_ms,
                preamble_ms,
                body_ms,
                pages,
                slowest_unit,
                slowest_unit_ms,
                page1_ms,
                first_run_ms,
                ..
            } => {
                let unit = slowest_unit.as_deref().map(|u| format!("{u} {slowest_unit_ms:.1} ms")).unwrap_or_default();
                let first = first_run_ms.map(|f| format!("{f:.0} ms")).unwrap_or_default();
                let _ = writeln!(
                    s,
                    "  {:<9} {:>5.0} ms {:>6.0} ms {:>5.0} ms {:>5}  {:<28} {:>5.1} ms {:>10}",
                    r.engine.command(),
                    total_ms,
                    preamble_ms,
                    body_ms,
                    pages,
                    unit,
                    page1_ms,
                    first
                );
            }
            Outcome::NotInstalled => {
                let _ = writeln!(s, "  {:<9} not installed", r.engine.command());
            }
            Outcome::Failed { exit_code, .. } => {
                let _ = writeln!(
                    s,
                    "  {:<9} build failed (exit {})",
                    r.engine.command(),
                    exit_code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
                );
            }
        }
    }
    let ok: Vec<(&Row, f64)> = c
        .rows
        .iter()
        .filter_map(|r| if let Outcome::Ok { total_ms, .. } = r.outcome { Some((r, total_ms)) } else { None })
        .collect();
    if ok.len() >= 2 {
        let fastest = ok.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        let slowest = ok.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        let _ = writeln!(
            s,
            "  hint: {} is fastest for this document; {} takes {:.0} ms more ({:.1}×).",
            fastest.0.engine.command(),
            slowest.0.engine.command(),
            slowest.1 - fastest.1,
            slowest.1 / fastest.1.max(0.001)
        );
    }
    for r in &c.rows {
        if let Outcome::Ok { total_ms, first_run_ms: Some(first), .. } = r.outcome
            && first > total_ms * 1.5
        {
            let _ = writeln!(
                s,
                "  note: {}'s first run took {:.0} ms (font cache build); the reported run is warm.",
                r.engine.command(),
                first
            );
        }
    }
    for r in &c.rows {
        if let Outcome::Failed { log_tail, .. } = &r.outcome {
            let _ = writeln!(s, "\n  {}: {}", r.engine.command(), log_tail.lines().collect::<Vec<_>>().join("\n    "));
        }
    }
    s
}
