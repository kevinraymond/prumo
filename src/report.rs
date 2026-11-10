//! Renderers for the reports. Text is for humans; JSON is the API (`"schema": 1`).

use crate::chart::Look;
use crate::model::{BuildModel, Frame, FrameKind, RunPhases, UnitRow};
use crate::records::Engine;
use crate::{Scaled, scaled_to_secs};
use std::fmt::Write as _;

/// How the engine's `\sys_timer:` measures. All three are wall clock with the zero at about
/// the start of the process (measured: the record file's birth time after spawn is within
/// 5 ms of the first record's timer on all three; file timestamps move in 4 ms steps).
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Timer {
    /// `wall`, or `unknown` for an engine prumo does not know.
    pub clock: &'static str,
    /// What `\sys_timer:` reads on this engine.
    pub source: &'static str,
}

pub fn timer_of(engine: Engine) -> Timer {
    match engine {
        Engine::PdfTex => Timer { clock: "wall", source: "\\pdfelapsedtime" },
        Engine::XeTex => Timer { clock: "wall", source: "\\elapsedtime" },
        // l3kernel (since 2020-10): epoch = os.gettimeofday() - os.clock() when its Lua code
        // loads, timer = gettimeofday() - epoch. Wall clock; os.clock() only back-dates the zero.
        Engine::LuaTex => Timer { clock: "wall", source: "os.gettimeofday() via l3kernel" },
        Engine::Other => Timer { clock: "unknown", source: "\\sys_timer:" },
    }
}

/// The phases as JSON. Every key is always present; `null` means the records (or the run)
/// lack what that phase needs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PhasesJson {
    pub startup_ms: f64,
    pub preamble_ms: Option<f64>,
    pub begin_document_ms: Option<f64>,
    pub typesetting_ms: Option<f64>,
    pub shipout_ms: f64,
    pub end_document_ms: Option<f64>,
    /// Part of `end_document_ms`: after the last page (the second `.aux` read).
    pub after_last_page_ms: Option<f64>,
    /// Process wall time minus the last record; `null` under latexmk or for an older cache.
    pub finalization_ms: Option<f64>,
    /// The timer at the last record: the sum of the six engine phases.
    pub last_record_ms: f64,
    /// Process wall time, spawn to exit: the sum of all seven.
    pub wall_ms: Option<f64>,
}

impl From<RunPhases> for PhasesJson {
    fn from(p: RunPhases) -> Self {
        PhasesJson {
            startup_ms: ms(p.startup),
            preamble_ms: p.preamble.map(ms),
            begin_document_ms: p.begin_document.map(ms),
            typesetting_ms: p.typesetting.map(ms),
            shipout_ms: ms(p.shipout),
            end_document_ms: p.end_document.map(ms),
            after_last_page_ms: p.after_last_page.map(ms),
            finalization_ms: p.finalization.map(ms),
            last_record_ms: ms(p.last_record),
            wall_ms: p.wall.map(ms),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct EnvJson {
    pub name: String,
    pub count: u32,
    /// Outermost instances of the name, `\begin` to `\end`, less shipouts inside.
    pub total_ms: f64,
    pub max_ms: f64,
}

pub fn env_json(m: &BuildModel) -> Vec<EnvJson> {
    m.env_table()
        .into_iter()
        .map(|r| EnvJson { name: r.name, count: r.count, total_ms: ms(r.total), max_ms: ms(r.max) })
        .collect()
}

/// The rows of the phase table: label, value, what it covers.
pub fn phase_rows(p: &RunPhases) -> [(&'static str, Option<Scaled>, &'static str); 7] {
    [
        ("startup", Some(p.startup), "engine start to the first record: format, engine setup"),
        ("preamble", p.preamble, "to \\begin{document}"),
        ("begin-document", p.begin_document, "\\AtBeginDocument hooks, the .aux"),
        ("typesetting", p.typesetting, "the body, less shipouts"),
        ("shipout", Some(p.shipout), "shipout hooks, writing each page"),
        ("end-document", p.end_document, "\\AtEndDocument, the last page, the .aux again"),
        ("finalization", p.finalization, "after the last record: closing the PDF, fonts"),
    ]
}

/// Why a run has no process wall time, for the finalization row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoWall {
    /// latexmk runs several passes and tools; no one process is the run.
    Latexmk,
    /// Records from a build prumo did not run (`prumo time output.prumo`).
    Records,
    /// A cached run from a prumo that did not time the process.
    OldCache,
}

/// The phase table as text. `no_wall` says why finalization is missing, when it is.
pub fn phases_text(m: &BuildModel, look: Option<Look>, no_wall: NoWall) -> String {
    let p = m.run_phases();
    let t = timer_of(m.engine);
    let mut s = String::new();
    let _ = writeln!(s, "\n-- phases --  {} clock ({}); finalization timed by prumo", t.clock, t.source);
    let total = p.wall.unwrap_or(p.last_record).max(1);
    let max = phase_rows(&p).iter().filter_map(|r| r.1).max().unwrap_or(1).max(1);
    for (label, v, what) in phase_rows(&p) {
        match v {
            Some(v) => {
                let bar = match look {
                    Some(l) => {
                        let b = format!("{:<12}", crate::chart::bar(v as f64 / max as f64, 12));
                        if l.color { format!("\x1b[36m{b}\x1b[0m") } else { b }
                    }
                    None => String::new(),
                };
                let _ = writeln!(
                    s,
                    "  {label:<15} {:>9.1} ms {:>4}%  {bar}{}{what}",
                    ms(v),
                    v * 100 / total,
                    if bar.is_empty() { "" } else { "  " }
                );
            }
            None => {
                let why = match label {
                    "finalization" => match no_wall {
                        NoWall::Latexmk => "not measured: latexmk runs several passes",
                        NoWall::Records => "not measured: prumo did not run this build",
                        NoWall::OldCache => "not measured: a cached run from an older prumo (--rebuild)",
                    },
                    _ => "the records lack this boundary",
                };
                let _ = writeln!(s, "  {label:<15} {:>12}  {why}", "-");
            }
        }
    }
    match p.wall {
        Some(w) => {
            let _ = writeln!(s, "  {:<15} {:>9.1} ms        process wall time, spawn to exit", "total", ms(w));
        }
        None => {
            let _ = writeln!(s, "  {:<15} {:>9.1} ms        to the last record", "total", ms(p.last_record));
        }
    }
    s
}

/// The environment table as text; empty when the run recorded none.
pub fn envs_text(m: &BuildModel, top: usize) -> String {
    let rows = m.env_table();
    if rows.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    let _ = writeln!(s, "\n-- environments --  \\begin to \\end, less shipouts; a nested one counts in its parent too");
    for r in rows.iter().take(top) {
        let _ = writeln!(s, "  {:>9.1} ms {:>5}×  slowest {:>7.1} ms  {}", ms(r.total), r.count, ms(r.max), r.name);
    }
    if rows.len() > top {
        let _ = writeln!(s, "  ... {} more (--json)", rows.len() - top);
    }
    s
}

/// Scaled seconds as milliseconds with microsecond resolution, for JSON.
pub fn ms(s: Scaled) -> f64 {
    (scaled_to_secs(s) * 1_000_000.0).round() / 1000.0
}

/// `prumo time --json`.
#[derive(Debug, serde::Serialize)]
pub struct TimeJson<'a> {
    pub schema: u32,
    pub job: &'a str,
    pub engine: String,
    pub format_date: &'a str,
    pub total_ms: f64,
    pub preamble_ms: f64,
    pub units: Vec<UnitJson>,
    pub pages: Vec<PageJson>,
    pub tree: Vec<TreeJson>,
    pub warnings: &'a [String],
    /// How the times were measured.
    pub timer: Timer,
    /// The run in consecutive phases (`startup` .. `finalization`).
    pub phases: PhasesJson,
    /// Watched environments, by time.
    pub environments: Vec<EnvJson>,
    /// Pages whose shipout started and was discarded by a package.
    pub discarded_shipouts: &'a [u32],
}

#[derive(Debug, serde::Serialize)]
pub struct UnitJson {
    pub kind: FrameKind,
    pub name: String,
    pub inclusive_ms: f64,
    pub exclusive_ms: f64,
    pub loads: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct PageJson {
    pub page: u32,
    pub ms: f64,
    /// Shipout part of `ms` (hooks: backgrounds, margins, links), when the probe recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipout_ms: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
pub struct TreeJson {
    pub kind: FrameKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
    pub inclusive_ms: f64,
    pub exclusive_ms: f64,
    pub children: Vec<TreeJson>,
}

fn tree_json(fr: &Frame) -> TreeJson {
    TreeJson {
        kind: fr.kind,
        name: fr.name.clone(),
        options: fr.options.clone(),
        inclusive_ms: ms(fr.inclusive()),
        exclusive_ms: ms(fr.exclusive()),
        children: fr.children.iter().map(tree_json).collect(),
    }
}

pub fn time_json<'a>(m: &'a BuildModel, job: &'a str) -> TimeJson<'a> {
    TimeJson {
        schema: crate::JSON_SCHEMA,
        job,
        engine: m.engine.to_string(),
        format_date: &m.format_date,
        total_ms: ms(m.total()),
        preamble_ms: ms(m.preamble()),
        units: m
            .unit_table()
            .into_iter()
            .map(|r| UnitJson {
                kind: r.kind,
                name: r.name,
                inclusive_ms: ms(r.inclusive),
                exclusive_ms: ms(r.exclusive),
                loads: r.loads,
            })
            .collect(),
        pages: m
            .page_split()
            .into_iter()
            .map(|p| PageJson { page: p.page, ms: ms(p.total()), shipout_ms: p.ship.map(ms) })
            .collect(),
        tree: m.root.children.iter().map(tree_json).collect(),
        warnings: &m.warnings,
        timer: timer_of(m.engine),
        phases: m.run_phases().into(),
        environments: env_json(m),
        discarded_shipouts: &m.discarded_ships,
    }
}

/// What `prumo time` groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeBy {
    /// Packages and classes, plus files not wrapped by one (the oracle's table).
    Unit,
    /// Packages and classes only.
    Package,
    /// Every frame by file name, units included under their file.
    File,
    Page,
}

/// `prumo time` as text: the oracle's layout plus a hint line per section.
/// What `time_text` shows besides the model.
#[derive(Debug, Clone, Copy)]
pub struct TextOptions {
    pub files_read: Option<usize>,
    pub by: TimeBy,
    pub top: usize,
    pub depth: usize,
    pub look: Option<Look>,
    /// Why there is no wall time, if there is none.
    pub no_wall: NoWall,
}

pub fn time_text(m: &BuildModel, job: &str, o: TextOptions) -> String {
    let TextOptions { files_read, by, top, depth, look, no_wall } = o;
    let mut s = String::new();
    let files = files_read.map(|n| format!(", {n} files read")).unwrap_or_default();
    let p = m.run_phases();
    let total = match p.wall {
        Some(w) => format!("{} ms wall", crate::scaled_to_ms(w)),
        None => format!("{} ms to the last record", crate::scaled_to_ms(p.last_record)),
    };
    let _ = writeln!(
        s,
        "== prumo report: {job}  ({total}, {} {}, {} pages{files})",
        m.engine_name,
        m.format_date,
        m.pages.len()
    );
    for w in &m.warnings {
        let _ = writeln!(s, "   warning: {w}");
    }
    s.push_str(&phases_text(m, look, no_wall));
    if let Some(look) = look
        && by != TimeBy::Page
    {
        let _ = writeln!(s, "\n-- {} --", if look.preamble { "preamble over time" } else { "build over time" });
        s.push_str(&crate::chart::timeline(m, depth, look));
    }
    let rows = m.unit_table();
    match by {
        TimeBy::Unit | TimeBy::Package | TimeBy::File => {
            let shown: Vec<&UnitRow> = rows.iter().filter(|r| by != TimeBy::Package || r.kind.is_unit()).collect();
            let _ = writeln!(s, "\n-- slowest units (exclusive) --");
            if let Some(look) = look {
                s.push_str(&crate::chart::unit_table(&shown, top, look));
            } else {
                for r in shown.iter().take(top) {
                    let _ = writeln!(
                        s,
                        "  {:>7.1} ms  {:>7.1} ms incl  {:<4} {}",
                        ms(r.exclusive),
                        ms(r.inclusive),
                        r.kind.to_string(),
                        r.name
                    );
                }
            }
            // The hint looks at every row, so a heavy bare file is not hidden by --by package.
            s.push_str(&unit_hint(m, &rows.iter().collect::<Vec<_>>()));
        }
        TimeBy::Page => {}
    }
    s.push_str(&envs_text(m, top));
    let _ = writeln!(s, "\n-- pages --");
    let split = m.page_split();
    let has_ship = split.iter().any(|p| p.ship.is_some());
    // With charts: one cell per page, then only the slowest pages as lines.
    let listed: Vec<&crate::model::PageSplit> = match look {
        Some(look) if by != TimeBy::Page => {
            s.push_str(&crate::chart::page_spark(m, look));
            let mut by_time: Vec<&crate::model::PageSplit> = split.iter().collect();
            by_time.sort_by_key(|p| std::cmp::Reverse(p.total()));
            let mut top5: Vec<_> = by_time.into_iter().take(5).collect();
            top5.sort_by_key(|p| p.page);
            if top5.len() < split.len() {
                let _ = writeln!(s, "  slowest {} of {} pages:", top5.len(), split.len());
            }
            top5
        }
        // Plain text: every page with --by page; otherwise the slowest --top of them, in
        // page order, so a book does not print a line per page.
        _ if by == TimeBy::Page || split.len() <= top => split.iter().collect(),
        _ => {
            let mut by_time: Vec<&crate::model::PageSplit> = split.iter().collect();
            by_time.sort_by_key(|p| std::cmp::Reverse(p.total()));
            let mut slowest: Vec<_> = by_time.into_iter().take(top).collect();
            slowest.sort_by_key(|p| p.page);
            let _ = writeln!(s, "  slowest {} of {} pages (--by page lists them all):", slowest.len(), split.len());
            slowest
        }
    };
    for p in listed {
        match p.ship {
            Some(sh) => {
                let _ = writeln!(
                    s,
                    "  page {}: {:.1} ms  (typesetting {:.1} ms, shipout {:.1} ms)",
                    p.page,
                    ms(p.typeset + sh),
                    ms(p.typeset),
                    ms(sh)
                );
            }
            None => {
                let _ = writeln!(s, "  page {}: {:.1} ms", p.page, ms(p.typeset));
            }
        }
    }
    if !m.discarded_ships.is_empty() {
        let pages: Vec<String> = m.discarded_ships.iter().map(u32::to_string).collect();
        let _ = writeln!(
            s,
            "  {} shipout{} discarded by a package before page {}; that time counts as typesetting",
            pages.len(),
            if pages.len() == 1 { "" } else { "s" },
            pages.join(", ")
        );
    }
    if has_ship {
        let ship_total: Scaled = split.iter().filter_map(|p| p.ship).sum();
        let body: Scaled = split.iter().map(|p| p.total()).sum();
        if body > 0 && ship_total * 4 > body {
            let _ = writeln!(
                s,
                "  hint: {}% of the body is spent in shipout, not typesetting: shipout hooks (page backgrounds, margins, links) and writing each page, its images included.",
                ship_total * 100 / body
            );
        }
    }
    if m.pages.len() > 1 {
        let times = m.page_times();
        if let Some(slowest) = times.iter().max_by_key(|p| p.time) {
            let first = times[0].time;
            if slowest.page == 1
                && times.len() > 1
                && first > 2 * times[1..].iter().map(|p| p.time).sum::<Scaled>() / (times.len() as Scaled - 1).max(1)
            {
                let _ = writeln!(
                    s,
                    "  hint: page 1 carries font loading and first-shipout setup ({:.1} ms); later pages average {:.1} ms",
                    ms(first),
                    ms(times[1..].iter().map(|p| p.time).sum::<Scaled>() / (times.len() as Scaled - 1))
                );
            } else {
                let _ = writeln!(s, "  hint: slowest page is {} at {:.1} ms", slowest.page, ms(slowest.time));
            }
        }
    }
    if by != TimeBy::Page && look.is_none() {
        let _ = writeln!(s, "\n-- load tree --");
        let mut lines = 0usize;
        m.root.walk(&mut |fr, d| {
            if d < depth {
                lines += 1;
                if lines <= 40 {
                    let _ = writeln!(
                        s,
                        "  {}{}  ({:.1} ms incl, {:.1} ms excl)",
                        "  ".repeat(d),
                        fr.name,
                        ms(fr.inclusive()),
                        ms(fr.exclusive())
                    );
                }
            }
        });
        if lines > 40 {
            let _ = writeln!(s, "  ... {} more (use --depth or --json)", lines - 40);
        }
    }
    s
}

/// The one-line hint under the unit table: the heaviest row (a file when a file beats every
/// package, as when XeTeX builds its font cache inside `size10.clo`), its share of the
/// preamble, and which package loaded it.
fn unit_hint(m: &BuildModel, rows: &[&UnitRow]) -> String {
    let preamble = m.preamble().max(1);
    let body = m.total().saturating_sub(m.preamble());
    // When the body dominates, the preamble share of a package is the wrong story.
    if body > 2 * preamble && !m.pages.is_empty() {
        let per_page = body / m.pages.len() as Scaled;
        return format!(
            "  hint: {}% of the time is the body: {} page{} at {:.0} ms each; the preamble is {} ms. See -- pages -- (and --by page).\n",
            body * 100 / m.total().max(1),
            m.pages.len(),
            if m.pages.len() == 1 { "" } else { "s" },
            ms(per_page),
            crate::scaled_to_ms(m.preamble())
        );
    }
    let top_unit = rows.iter().find(|r| r.kind.is_unit());
    let top_file = rows.iter().find(|r| !r.kind.is_unit() && !r.name.ends_with(".tex"));
    let top = match (top_unit, top_file) {
        (Some(u), Some(f)) if f.exclusive > u.exclusive => f,
        (Some(u), _) => u,
        (None, Some(f)) => f,
        (None, None) => return String::new(),
    };
    let share = top.inclusive * 100 / preamble;
    let loader = parent_of(&m.root, &top.name, top.kind).map(|p| format!("; it is loaded by {p}")).unwrap_or_default();
    let what = if top.kind.is_unit() { "" } else { "file " };
    // Loaded from inside the main file (Overleaf), the probe never saw it open.
    let Some(main) = m.main_file() else {
        return format!("  hint: {what}{} is {share}% of your preamble{loader}.\n", top.name);
    };
    format!(
        "  hint: {what}{} is {share}% of your preamble{loader}. {:.0} ms is in {} itself, outside any package.\n",
        top.name,
        ms(main.exclusive()),
        main.name
    )
}

fn parent_of(root: &Frame, name: &str, kind: FrameKind) -> Option<String> {
    fn go(fr: &Frame, name: &str, kind: FrameKind) -> Option<String> {
        for c in &fr.children {
            if c.name == name && c.kind == kind {
                return Some(match fr.kind {
                    FrameKind::Job => "the command line".into(),
                    FrameKind::Cls => format!("{}.cls", fr.name),
                    _ => fr.name.clone(),
                });
            }
            if let Some(p) = go(c, name, kind) {
                return Some(p);
            }
        }
        None
    }
    go(root, name, kind)
}
