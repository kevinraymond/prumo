//! `prumo compare`: the same document on every engine this machine has, one table.
//!
//! The requester's question was "maybe I could find a compiler that is two seconds faster";
//! the honest answer is specific to his documents, so this runs them. Each engine gets its
//! own cache (`compare/<engine>/<job>/` under the document's cache directory). The first run on XeTeX and LuaTeX pays for
//! font caches (findings 20 and 26), so by default every engine runs twice and the second
//! run is reported; the first is shown alongside when it differs. With three runs or more,
//! the first is still the warm-up and the reported run is the median of the others by wall
//! time (by the last record when wall time is not measured).

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
        /// The reported run in phases, startup to finalization.
        phases: Box<crate::report::PhasesJson>,
        /// How the engine's timer measures.
        timer: crate::report::Timer,
        /// Which run is reported (1-based) and the wall times of every run, for the spread.
        reported_run: u32,
        runs_wall_ms: Vec<Option<f64>>,
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
    /// Environments the probe times.
    pub environments: Vec<String>,
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
        let mut warm: Vec<(u32, BuildModel)> = Vec::new();
        let mut walls: Vec<Option<f64>> = Vec::new();
        let mut failed: Option<Outcome> = None;
        for i in 0..opts.runs.max(1) {
            progress(&format!("{}: run {}/{}", engine.command(), i + 1, opts.runs.max(1)));
            let out = run::run(&RunOptions {
                latexmk: opts.latexmk,
                out: Some(out_dir.clone()),
                env: opts.env.clone(),
                environments: opts.environments.clone(),
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
            walls.push(model.wall.map(ms));
            if i == 0 {
                first_total = Some(total_of(&model));
            }
            if i > 0 || opts.runs <= 1 {
                warm.push((i + 1, model));
            }
        }
        if let Some(f) = failed {
            rows.push(Row { engine, outcome: f });
            continue;
        }
        // The median warm run by wall time (by the last record without one); with two runs,
        // the second.
        warm.sort_by_key(|(_, m)| m.wall.unwrap_or(m.end));
        let mid = warm.len().saturating_sub(1) / 2;
        let Some((reported_run, model)) = warm.into_iter().nth(mid) else { continue };
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
                phases: Box::new(model.run_phases().into()),
                timer: crate::report::timer_of(model.engine),
                reported_run,
                runs_wall_ms: walls,
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
        match c.runs {
            1 => "",
            2 => ", the first is a warm-up, the second is reported",
            _ => ", the first is a warm-up, the median of the rest is reported",
        }
    );
    // The wall time is what the user waits for: startup and finalization included. Runs
    // from before prumo measured it fall back to the records' span, marked with *.
    let wall_or_total = |o: &Outcome| match o {
        Outcome::Ok { phases, total_ms, .. } => Some(phases.wall_ms.map(|w| (w, true)).unwrap_or((*total_ms, false))),
        _ => None,
    };
    let _ = writeln!(
        s,
        "  {:<9} {:>10} {:>5}  {:<28} {:>8} {:>12}",
        "engine", "wall", "pages", "slowest unit (excl)", "page 1", "first run"
    );
    for r in &c.rows {
        match &r.outcome {
            Outcome::Ok { pages, slowest_unit, slowest_unit_ms, page1_ms, first_run_ms, runs_wall_ms, .. } => {
                let (t, wall) = wall_or_total(&r.outcome).unwrap_or((0.0, false));
                let unit = slowest_unit.as_deref().map(|u| format!("{u} {slowest_unit_ms:.1} ms")).unwrap_or_default();
                let first = match (runs_wall_ms.first().copied().flatten(), first_run_ms) {
                    (Some(w), Some(_)) => format!("{w:.0} ms"),
                    (None, Some(f)) => format!("{f:.0} ms*"),
                    _ => String::new(),
                };
                let _ = writeln!(
                    s,
                    "  {:<9} {:>7.0} ms{} {:>5}  {:<28} {:>5.1} ms {:>12}",
                    r.engine.command(),
                    t,
                    if wall { " " } else { "*" },
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
    s.push_str(&phase_table(c));
    let ok: Vec<(&Row, f64, bool)> =
        c.rows.iter().filter_map(|r| wall_or_total(&r.outcome).map(|(t, w)| (r, t, w))).collect();
    if ok.iter().any(|(_, _, w)| !w) {
        let _ = writeln!(
            s,
            "  * no wall time for this run (latexmk, or a cache from an older prumo): first record to last"
        );
    }
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
        if let Outcome::Ok { runs_wall_ms, first_run_ms: Some(first_total), .. } = &r.outcome {
            let (warm, _) = wall_or_total(&r.outcome).unwrap_or((0.0, false));
            let first = runs_wall_ms.first().copied().flatten().unwrap_or(*first_total);
            if first > warm * 1.5 {
                let _ = writeln!(
                    s,
                    "  note: {}'s first run took {:.0} ms (font cache build); the reported run is warm.",
                    r.engine.command(),
                    first
                );
            }
        }
    }
    for r in &c.rows {
        if let Outcome::Failed { log_tail, .. } = &r.outcome {
            let _ = writeln!(s, "\n  {}: {}", r.engine.command(), log_tail.lines().collect::<Vec<_>>().join("\n    "));
        }
    }
    s
}

/// Phases down, engines across: where each engine's wall time goes.
fn phase_table(c: &Comparison) -> String {
    use std::fmt::Write as _;
    let ok: Vec<(&Row, &crate::report::PhasesJson, &crate::report::Timer)> = c
        .rows
        .iter()
        .filter_map(|r| match &r.outcome {
            Outcome::Ok { phases, timer, .. } => Some((r, &**phases, timer)),
            _ => None,
        })
        .collect();
    if ok.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    let sources: Vec<String> = ok.iter().map(|(r, _, t)| format!("{} {}", r.engine.command(), t.source)).collect();
    let _ = writeln!(s, "\n-- phases (ms) --  wall clock: {}", sources.join(", "));
    let _ = write!(s, "  {:<15}", "");
    for (r, _, _) in &ok {
        let _ = write!(s, " {:>10}", r.engine.command());
    }
    s.push('\n');
    type Get = fn(&crate::report::PhasesJson) -> Option<f64>;
    let rows: [(&str, Get); 8] = [
        ("startup", |p| Some(p.startup_ms)),
        ("preamble", |p| p.preamble_ms),
        ("begin-document", |p| p.begin_document_ms),
        ("typesetting", |p| p.typesetting_ms),
        ("shipout", |p| Some(p.shipout_ms)),
        ("end-document", |p| p.end_document_ms),
        ("finalization", |p| p.finalization_ms),
        ("total (wall)", |p| p.wall_ms),
    ];
    for (label, get) in rows {
        let _ = write!(s, "  {label:<15}");
        for (_, p, _) in &ok {
            match get(p) {
                Some(v) => {
                    let _ = write!(s, " {v:>10.1}");
                }
                None => {
                    let _ = write!(s, " {:>10}", "-");
                }
            }
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{PhasesJson, timer_of};

    fn ok(engine: EngineCmd, total_ms: f64, startup_ms: f64, wall_ms: Option<f64>) -> Row {
        let phases = PhasesJson {
            startup_ms,
            preamble_ms: Some(10.0),
            begin_document_ms: Some(1.0),
            typesetting_ms: Some(1.0),
            shipout_ms: 1.0,
            end_document_ms: Some(1.0),
            after_last_page_ms: Some(0.5),
            finalization_ms: wall_ms.map(|w| w - startup_ms - total_ms),
            last_record_ms: startup_ms + total_ms,
            wall_ms,
        };
        Row {
            engine,
            outcome: Outcome::Ok {
                total_ms,
                preamble_ms: 10.0,
                body_ms: total_ms - 10.0,
                pages: 1,
                slowest_unit: Some("article".into()),
                slowest_unit_ms: 3.0,
                page1_ms: 1.0,
                later_pages_ms: None,
                first_run_ms: Some(total_ms),
                phases: Box::new(phases),
                timer: timer_of(crate::records::Engine::PdfTex),
                reported_run: 2,
                runs_wall_ms: vec![wall_ms.map(|w| w * 20.0), wall_ms],
            },
        }
    }

    /// The minimal document on 2026-11-09: LuaLaTeX has the shortest span between its first
    /// and last records and the longest wall time. The ranking follows the wall time.
    #[test]
    fn engines_are_ranked_by_wall_time() {
        let c = Comparison {
            schema: 1,
            job: "main".into(),
            runs: 4,
            rows: vec![
                ok(EngineCmd::Pdflatex, 91.0, 57.0, Some(202.0)),
                ok(EngineCmd::Lualatex, 80.0, 360.0, Some(475.0)),
            ],
        };
        let t = text(&c);
        assert!(t.contains("hint: pdflatex is fastest"), "{t}");
        assert!(t.contains("lualatex takes 273 ms more"), "{t}");
        assert!(t.contains("-- phases (ms) --") && t.contains("finalization"), "{t}");
        assert!(t.contains("first run took 9500 ms"), "the first run's wall time: {t}");
        assert!(!t.contains('*'), "every row has a wall time: {t}");
        // A row without a wall time (latexmk) falls back to the records' span, marked.
        let c = Comparison { rows: vec![ok(EngineCmd::Xelatex, 100.0, 80.0, None)], ..c };
        let t = text(&c);
        assert!(t.contains("100 ms*") && t.contains("* no wall time"), "{t}");
    }
}
