//! Renderers for the reports. Text is for humans; JSON is the API (`"schema": 1`).

use crate::chart::Look;
use crate::model::{BuildModel, Frame, FrameKind, UnitRow};
use crate::{Scaled, scaled_to_secs};
use std::fmt::Write as _;

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
pub fn time_text(
    m: &BuildModel,
    job: &str,
    files_read: Option<usize>,
    by: TimeBy,
    top: usize,
    depth: usize,
    look: Option<Look>,
) -> String {
    let mut s = String::new();
    let files = files_read.map(|n| format!(", {n} files read")).unwrap_or_default();
    let _ = writeln!(
        s,
        "== prumo report: {job}  (total {} ms, preamble {} ms, {} pages{files})",
        crate::scaled_to_ms(m.total()),
        crate::scaled_to_ms(m.preamble()),
        m.pages.len()
    );
    for w in &m.warnings {
        let _ = writeln!(s, "   warning: {w}");
    }
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
        _ => split.iter().collect(),
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
    if has_ship {
        let ship_total: Scaled = split.iter().filter_map(|p| p.ship).sum();
        let body: Scaled = split.iter().map(|p| p.total()).sum();
        if body > 0 && ship_total * 4 > body {
            let _ = writeln!(
                s,
                "  hint: {}% of the body is spent in shipout hooks (page backgrounds, margins, links), not typesetting.",
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
    let main_excl = m.main_file().map(Frame::exclusive).unwrap_or(0);
    format!(
        "  hint: {what}{} is {share}% of your preamble{loader}. {:.0} ms is in {} itself, outside any package.\n",
        top.name,
        ms(main_excl),
        m.main_file().map(|f| f.name.as_str()).unwrap_or("the main file")
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
