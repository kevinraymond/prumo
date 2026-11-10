//! Record stream → `BuildModel`: the frame tree, phases and pages.
//!
//! The five rules from `docs/03-architecture.md`, all mirrored from the oracle
//! (`scripts/prumo_report.py`, `build_tree` and `collapse_unit_files`):
//!
//! 1. `file+`/`pkg+`/`cls+` push a frame; the matching `-` pops it.
//! 2. A `pkg`/`cls` frame wraps the file frame of its own `.sty`/`.cls`: **collapse** that
//!    file frame into the unit, splicing its children up one level.
//! 3. `\end{document}` never returns from `\input{main}`; frames still open at the last
//!    record close at that timestamp.
//! 4. `opts` directly after `pkg+` attaches to that frame.
//! 5. `page N` events are leaves of the job; page time = time since the previous page, or
//!    since `docstart` (`begindoc` in older records) for the first.
//!
//! On top of the tree: the run's phases (`run_phases`), the watched environments
//! (`env+`/`env-`, paired by name) and shipouts a package discarded (`ship` with no `page`).

use crate::Scaled;
use crate::records::{Engine, Event, Record, UnitKind};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameKind {
    Job,
    Cls,
    Pkg,
    File,
}

impl FrameKind {
    fn of(kind: UnitKind) -> FrameKind {
        match kind {
            UnitKind::File => FrameKind::File,
            UnitKind::Pkg => FrameKind::Pkg,
            UnitKind::Cls => FrameKind::Cls,
        }
    }

    pub fn is_unit(self) -> bool {
        matches!(self, FrameKind::Pkg | FrameKind::Cls)
    }
}

impl fmt::Display for FrameKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FrameKind::Job => "job",
            FrameKind::Cls => "cls",
            FrameKind::Pkg => "pkg",
            FrameKind::File => "file",
        })
    }
}

/// One node of the load tree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Frame {
    pub kind: FrameKind,
    /// `siunitx` / `t1pcr.fd` / `main.tex`.
    pub name: String,
    /// Raw option list for packages, when the kernel recorded one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
    pub start: Scaled,
    pub end: Scaled,
    pub children: Vec<Frame>,
    /// Record line of the opening event; 0 for the job root.
    #[serde(skip)]
    pub line: usize,
}

impl Frame {
    pub fn inclusive(&self) -> Scaled {
        self.end.saturating_sub(self.start)
    }

    /// Inclusive minus the children's inclusive times.
    pub fn exclusive(&self) -> Scaled {
        let kids: Scaled = self.children.iter().map(Frame::inclusive).sum();
        debug_assert!(kids <= self.inclusive(), "children outlast parent {}", self.name);
        self.inclusive().saturating_sub(kids)
    }

    /// Depth-first, pre-order walk over every frame below this one (not this one).
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Frame, usize)) {
        fn go<'a>(fr: &'a Frame, depth: usize, f: &mut dyn FnMut(&'a Frame, usize)) {
            for c in &fr.children {
                f(c, depth);
                go(c, depth + 1, f);
            }
        }
        go(self, 0, f)
    }

    /// First frame below this one with the given name, pre-order.
    pub fn find(&self, name: &str) -> Option<&Frame> {
        let mut found = None;
        self.walk(&mut |fr, _| {
            if found.is_none() && fr.name == name {
                found = Some(fr);
            }
        });
        found
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Phases {
    /// `begindocument/before`: the end of the preamble proper (None for old records).
    pub predoc: Option<Scaled>,
    pub begindoc: Option<Scaled>,
    /// `begindocument/end`: `\begin{document}` is done (probe v0.2 and later).
    pub docstart: Option<Scaled>,
    pub enddoc: Option<Scaled>,
    /// `enddocument/afterlastpage` (probe v0.2 and later).
    pub lastpage: Option<Scaled>,
}

/// One `\begin{name}` .. `\end{name}` of a watched environment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EnvInstance {
    pub name: String,
    pub start: Scaled,
    pub end: Scaled,
    /// Inside another instance of the same name (its time is already in the outer one).
    pub nested: bool,
}

/// The run cut into consecutive phases. Each phase leaves out the shipouts inside it, and
/// `shipout` holds them all, so the six engine phases add up to the timer at the last
/// record. `None` where the records lack a boundary (an older probe, a failed build).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunPhases {
    /// Timer zero (about process start) to the first record: format loading, engine setup.
    pub startup: Scaled,
    /// First record to `predoc`.
    pub preamble: Option<Scaled>,
    /// `predoc` to `docstart`: `begindocument/before` loads, the `.aux`, `\AtBeginDocument`.
    pub begin_document: Option<Scaled>,
    /// `docstart` to `enddoc`, less shipouts.
    pub typesetting: Option<Scaled>,
    /// Every `ship`..`page`, wherever it happened.
    pub shipout: Scaled,
    /// `enddoc` to the last record, less shipouts: `\AtEndDocument`, the last pages, the
    /// second `.aux` read.
    pub end_document: Option<Scaled>,
    /// The part of `end_document` after the last page (`lastpage` to the last record).
    pub after_last_page: Option<Scaled>,
    /// After the last record until the process exits: the engine closes the PDF (fonts,
    /// xdvipdfmx on XeTeX). Process wall time minus the last record; only when the CLI
    /// timed a single engine process.
    pub finalization: Option<Scaled>,
    /// The timer at the last record: the sum of the six engine phases.
    pub last_record: Scaled,
    /// Process wall time, when measured.
    pub wall: Option<Scaled>,
}

/// One row of the environment table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRow {
    pub name: String,
    /// Instances, nested ones and repeated passes (tabularx typesets its body several
    /// times) included.
    pub count: u32,
    /// Outermost instances of this name only, less the shipouts inside them.
    pub total: Scaled,
    /// The slowest single instance.
    pub max: Scaled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Page {
    pub number: u32,
    /// `\sys_timer:` at `shipout/after`.
    pub time: Scaled,
    /// `\sys_timer:` at `shipout/before`, when the probe recorded it.
    pub ship_start: Option<Scaled>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BuildModel {
    pub engine: Engine,
    pub engine_name: String,
    pub format_date: String,
    /// Timer at the `start` record.
    pub start: Scaled,
    /// Timer at the last record.
    pub end: Scaled,
    pub phases: Phases,
    /// The job: its children are the main file (and anything loaded before it).
    pub root: Frame,
    pub pages: Vec<Page>,
    /// Watched environments, in the order they closed.
    pub envs: Vec<EnvInstance>,
    /// Page numbers of shipouts that started (`ship`) and never shipped (`page`): a package
    /// discarded the box.
    pub discarded_ships: Vec<u32>,
    /// Process wall time of the engine run, when the CLI measured one (not set from the
    /// records; `run.json` carries it).
    pub wall: Option<Scaled>,
    /// Oddities seen while building: not errors, but worth a line in the report.
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("{file}:{line}: {msg}")]
    Line { file: String, line: usize, msg: String },
    #[error("{file}: no records")]
    Empty { file: String },
    /// The records failed the structural checks (`records::validate`).
    #[error(transparent)]
    Records(#[from] crate::records::RecordsError),
}

/// One row of the per-unit table: a package or class, or a file not wrapped by one.
/// Rows aggregate every load of the same unit (`loads` > 1 when a `.fd` is read twice).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UnitRow {
    pub kind: FrameKind,
    pub name: String,
    pub inclusive: Scaled,
    pub exclusive: Scaled,
    pub loads: u32,
}

/// One page's time: from the previous shipout (or `\begin{document}`) to its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTime {
    pub page: u32,
    pub time: Scaled,
}

/// One page's time split at the shipout. `ship` is `None` when the records have no `ship`
/// events (an older probe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSplit {
    pub page: u32,
    pub typeset: Scaled,
    pub ship: Option<Scaled>,
}

impl PageSplit {
    /// Typesetting and shipout together: the page's whole time.
    pub fn total(&self) -> Scaled {
        self.typeset + self.ship.unwrap_or(0)
    }
}

impl BuildModel {
    /// Build from parsed records. `file` is for error messages only.
    pub fn from_records(records: &[Record], file: &str) -> Result<BuildModel, ModelError> {
        // Structure first (start record, timers never backwards, balanced opens), so the
        // arithmetic below never sees a bad file.
        crate::records::validate(records, file)?;
        let err = |line: usize, msg: String| ModelError::Line { file: file.to_string(), line, msg };
        let first = records.first().ok_or_else(|| ModelError::Empty { file: file.to_string() })?;
        let Event::Start { engine, engine_name, format_date } = &first.event else {
            return Err(err(first.line, "first record is not `start`".into()));
        };
        let start = first.time;
        let mut root = Frame {
            kind: FrameKind::Job,
            name: "<job>".into(),
            options: None,
            start,
            end: start,
            children: vec![],
            line: 0,
        };
        let mut stack: Vec<Frame> = vec![];
        let mut phases = Phases::default();
        let mut pages = Vec::new();
        let mut pending_ship: Option<(u32, Scaled)> = None;
        let mut discarded_ships = Vec::new();
        let mut envs: Vec<EnvInstance> = Vec::new();
        let mut env_stack: Vec<(String, Scaled)> = Vec::new();
        let mut warnings = Vec::new();
        let mut last = start;
        for r in records {
            last = r.time;
            match &r.event {
                Event::Start { .. } => {
                    if r.line != first.line {
                        return Err(err(r.line, "second `start` record".into()));
                    }
                }
                Event::Open { kind, name } => stack.push(Frame {
                    kind: FrameKind::of(*kind),
                    name: name.clone(),
                    options: None,
                    start: r.time,
                    end: r.time,
                    children: vec![],
                    line: r.line,
                }),
                Event::Close { kind, name } => {
                    let Some(mut fr) = stack.pop() else {
                        return Err(err(r.line, format!("{kind}- {name} with nothing open")));
                    };
                    if fr.kind != FrameKind::of(*kind) || fr.name != *name {
                        return Err(err(
                            r.line,
                            format!("{kind}- {name} closes {} {} opened at line {}", fr.kind, fr.name, fr.line),
                        ));
                    }
                    fr.end = r.time;
                    stack.last_mut().map(|p| &mut p.children).unwrap_or(&mut root.children).push(fr);
                }
                Event::Opts(o) => match stack.last_mut() {
                    Some(fr) if fr.kind == FrameKind::Pkg => fr.options = Some(o.clone()),
                    _ => warnings.push(format!("line {}: opts outside a package frame", r.line)),
                },
                Event::PreDoc => phases.predoc = Some(r.time),
                Event::BeginDoc => phases.begindoc = Some(r.time),
                Event::DocStart => phases.docstart = Some(r.time),
                Event::EndDoc => phases.enddoc = Some(r.time),
                Event::LastPage => phases.lastpage = Some(r.time),
                Event::Ship(n) => {
                    // A second `ship` before any `page`: the first box was discarded.
                    if let Some((m, _)) = pending_ship.replace((*n, r.time)) {
                        discarded_ships.push(m);
                    }
                }
                Event::Page(n) => {
                    let ship_start = match pending_ship.take() {
                        Some((m, t)) if m == *n => Some(t),
                        Some((m, _)) => {
                            discarded_ships.push(m);
                            None
                        }
                        None => None,
                    };
                    pages.push(Page { number: *n, time: r.time, ship_start });
                }
                Event::EnvOpen(name) => env_stack.push((name.clone(), r.time)),
                Event::EnvClose(name) => {
                    // The innermost open instance of this name; an unmatched close is ignored.
                    if let Some(i) = env_stack.iter().rposition(|(n, _)| n == name) {
                        let (name, start) = env_stack.remove(i);
                        let nested = env_stack.iter().any(|(n, _)| *n == name);
                        envs.push(EnvInstance { name, start, end: r.time, nested });
                    }
                }
                Event::End => {}
            }
        }
        // Rule 3: close what is still open at the last timestamp.
        while let Some(mut fr) = stack.pop() {
            fr.end = last;
            if fr.name != "main.tex" && !fr.name.ends_with(".tex") {
                warnings.push(format!("line {}: {} {} never closed", fr.line, fr.kind, fr.name));
            }
            stack.last_mut().map(|p| &mut p.children).unwrap_or(&mut root.children).push(fr);
        }
        root.end = last;
        if let Some((m, _)) = pending_ship {
            discarded_ships.push(m);
        }
        // Environments still open at the last record (a build that stopped) close there.
        for (i, (name, t)) in env_stack.iter().enumerate() {
            let nested = env_stack[..i].iter().any(|(n, _)| n == name);
            envs.push(EnvInstance { name: name.clone(), start: *t, end: last, nested });
        }
        collapse_unit_files(&mut root);
        Ok(BuildModel {
            engine: *engine,
            engine_name: engine_name.clone(),
            format_date: format_date.clone(),
            start,
            end: last,
            phases,
            root,
            pages,
            envs,
            discarded_ships,
            wall: None,
            warnings,
        })
    }

    /// Where the body starts: `docstart`, else `begindoc` (older records).
    pub fn body_start(&self) -> Option<Scaled> {
        self.phases.docstart.or(self.phases.begindoc)
    }

    /// Completed shipouts as `(ship, page)` timer pairs.
    pub fn shipouts(&self) -> impl Iterator<Item = (Scaled, Scaled)> + '_ {
        self.pages.iter().filter_map(|p| p.ship_start.map(|s| (s, p.time)))
    }

    /// Shipout time inside `[a, b]`.
    fn ship_in(&self, a: Scaled, b: Scaled) -> Scaled {
        self.shipouts().map(|(s, e)| e.min(b).saturating_sub(s.max(a))).sum()
    }

    /// `[a, b]` less the shipouts inside it; `None` when a boundary is missing.
    fn span(&self, a: Option<Scaled>, b: Option<Scaled>) -> Option<Scaled> {
        let (a, b) = (a?, b?);
        Some(b.saturating_sub(a).saturating_sub(self.ship_in(a, b)))
    }

    /// The run's phases (the oracle's `run_phases`), plus finalization when `wall` is set.
    pub fn run_phases(&self) -> RunPhases {
        let predoc = self.phases.predoc.or(self.phases.begindoc);
        let docstart = self.body_start();
        let enddoc = self.phases.enddoc;
        // A build that stopped in the body: typesetting runs to the last record.
        let body_end = enddoc.or(docstart.map(|_| self.end));
        RunPhases {
            startup: self.start,
            preamble: self.span(Some(self.start), predoc),
            begin_document: self.span(predoc, docstart),
            typesetting: self.span(docstart, body_end),
            shipout: self.shipouts().map(|(s, e)| e.saturating_sub(s)).sum(),
            end_document: self.span(enddoc, Some(self.end)),
            after_last_page: self.span(self.phases.lastpage, Some(self.end)),
            finalization: self.wall.map(|w| w.saturating_sub(self.end)),
            last_record: self.end,
            wall: self.wall,
        }
    }

    /// Per environment name: instances, time, slowest instance; by time, descending.
    pub fn env_table(&self) -> Vec<EnvRow> {
        let mut rows: Vec<EnvRow> = Vec::new();
        for e in &self.envs {
            let dt = e.end.saturating_sub(e.start).saturating_sub(self.ship_in(e.start, e.end));
            let i = match rows.iter().position(|r| r.name == e.name) {
                Some(i) => i,
                None => {
                    rows.push(EnvRow { name: e.name.clone(), count: 0, total: 0, max: 0 });
                    rows.len() - 1
                }
            };
            let r = &mut rows[i];
            r.count += 1;
            if !e.nested {
                r.total += dt;
            }
            r.max = r.max.max(dt);
        }
        rows.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.name.cmp(&b.name)));
        rows
    }

    pub fn total(&self) -> Scaled {
        self.end.saturating_sub(self.start)
    }

    /// Time from engine start to `\begin{document}`; the whole run if there was none.
    pub fn preamble(&self) -> Scaled {
        self.phases.begindoc.unwrap_or(self.end) - self.start
    }

    /// Classes and packages that started loading before `\begin{document}`: what the
    /// folded preamble row stands for.
    pub fn preamble_units(&self) -> usize {
        let begindoc = self.phases.begindoc.unwrap_or(self.end);
        let mut n = 0;
        self.root.walk(&mut |fr, _| {
            if fr.kind.is_unit() && fr.start < begindoc {
                n += 1;
            }
        });
        n
    }

    /// Rule 5: per-page time. The first page counts from `docstart`, else `begindoc` (0 if
    /// there was neither, as in the oracle).
    pub fn page_times(&self) -> Vec<PageTime> {
        let mut prev = self.body_start();
        self.pages
            .iter()
            .map(|p| {
                let dt = prev.map_or(0, |t| p.time.saturating_sub(t));
                prev = Some(p.time);
                PageTime { page: p.number, time: dt }
            })
            .collect()
    }

    /// Rows in the oracle's order: by exclusive time, descending, ties in first-load order.
    pub fn unit_table(&self) -> Vec<UnitRow> {
        let mut rows: Vec<UnitRow> = Vec::new();
        self.root.walk(&mut |fr, _| {
            let kind = if fr.kind.is_unit() { fr.kind } else { FrameKind::File };
            match rows.iter_mut().find(|r| r.kind == kind && r.name == fr.name) {
                Some(r) => {
                    r.inclusive += fr.inclusive();
                    r.exclusive += fr.exclusive();
                    r.loads += 1;
                }
                None => rows.push(UnitRow {
                    kind,
                    name: fr.name.clone(),
                    inclusive: fr.inclusive(),
                    exclusive: fr.exclusive(),
                    loads: 1,
                }),
            }
        });
        rows.sort_by_key(|r| std::cmp::Reverse(r.exclusive));
        rows
    }

    /// Per page, the time split at the shipout: typesetting first, then the shipout hooks.
    pub fn page_split(&self) -> Vec<PageSplit> {
        let mut prev = self.body_start();
        self.pages
            .iter()
            .map(|p| {
                let total = prev.map_or(0, |t| p.time.saturating_sub(t));
                prev = Some(p.time);
                match p.ship_start {
                    Some(s) => PageSplit {
                        page: p.number,
                        typeset: total.saturating_sub(p.time.saturating_sub(s)),
                        ship: Some(p.time.saturating_sub(s)),
                    },
                    None => PageSplit { page: p.number, typeset: total, ship: None },
                }
            })
            .collect()
    }

    /// The main file frame: the first `.tex` child of the job. None when the probe was
    /// loaded from inside the main file (Overleaf), which then has no frame of its own.
    pub fn main_file(&self) -> Option<&Frame> {
        self.root.children.iter().find(|f| f.kind == FrameKind::File && f.name.ends_with(".tex"))
    }
}

/// Rule 2. Mirrors the oracle exactly: only direct children are considered, in order, and
/// the recursion runs over the children after splicing.
fn collapse_unit_files(fr: &mut Frame) {
    if fr.kind.is_unit() {
        let old = std::mem::take(&mut fr.children);
        for c in old {
            if c.kind == FrameKind::File && splitext_stem(&c.name) == fr.name {
                fr.children.extend(c.children);
            } else {
                fr.children.push(c);
            }
        }
    }
    for c in &mut fr.children {
        collapse_unit_files(c);
    }
}

/// Python's `os.path.splitext(name)[0]`: strip the last extension of the basename, where a
/// leading-dot name (`.hidden`) has no extension.
pub(crate) fn splitext_stem(name: &str) -> &str {
    let base_start = name.rfind('/').map_or(0, |i| i + 1);
    let base = &name[base_start..];
    let first_non_dot = base.find(|c| c != '.').unwrap_or(base.len());
    match base[first_non_dot..].rfind('.') {
        Some(i) => &name[..base_start + first_non_dot + i],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::parse_str;

    fn model(text: &str) -> BuildModel {
        BuildModel::from_records(&parse_str(text, "t").unwrap(), "t").unwrap()
    }

    #[test]
    fn splitext_matches_python() {
        assert_eq!(splitext_stem("siunitx.sty"), "siunitx");
        assert_eq!(splitext_stem("size10.clo"), "size10");
        assert_eq!(splitext_stem("a.b.c"), "a.b");
        assert_eq!(splitext_stem(".hidden"), ".hidden");
        assert_eq!(splitext_stem("..x"), "..x");
        assert_eq!(splitext_stem("dir.d/file"), "dir.d/file");
        assert_eq!(splitext_stem("dir.d/file.tex"), "dir.d/file");
        assert_eq!(splitext_stem("noext"), "noext");
    }

    #[test]
    fn builds_tree_and_collapses_unit_files() {
        let m = model(
            "start 100 0 pdftex 2026-11-01
file+ 100 0 main.tex
cls+ 110 1 article
file+ 112 1 article.cls
file+ 120 2 size10.clo
file- 130 2 size10.clo
file- 140 1 article.cls
cls- 142 1 article
pkg+ 150 1 siunitx
opts 150 1 per-mode=symbol
file+ 152 1 siunitx.sty
pkg+ 160 2 translations
opts 160 2
file+ 162 2 translations.sty
file- 180 2 translations.sty
pkg- 182 2 translations
file- 200 1 siunitx.sty
pkg- 202 1 siunitx
begindoc 300 1
page 400 1 1
enddoc 450 1
page 460 1 2
end 470 1
",
        );
        assert_eq!(m.total(), 370);
        assert_eq!(m.preamble(), 200);
        let main = m.main_file().unwrap();
        assert_eq!(main.name, "main.tex");
        assert_eq!(main.end, 470, "rule 3: main.tex closes at the last record");
        let names: Vec<_> = main.children.iter().map(|c| format!("{} {}", c.kind, c.name)).collect();
        assert_eq!(names, ["cls article", "pkg siunitx"]);
        let article = &main.children[0];
        assert_eq!(article.children.len(), 1, "article.cls collapsed; size10.clo moved up");
        assert_eq!(article.children[0].name, "size10.clo");
        assert_eq!(article.inclusive(), 32);
        assert_eq!(article.exclusive(), 22);
        let siunitx = &main.children[1];
        assert_eq!(siunitx.options.as_deref(), Some("per-mode=symbol"));
        assert_eq!(siunitx.children[0].name, "translations");
        assert_eq!(siunitx.children[0].options.as_deref(), Some(""));
        assert_eq!(siunitx.exclusive(), 52 - 22);
        assert_eq!(
            m.page_times(),
            vec![PageTime { page: 1, time: 100 }, PageTime { page: 2, time: 60 }],
            "page 2 counts from page 1, not enddoc"
        );
        let rows = m.unit_table();
        assert_eq!(rows[0].name, "main.tex");
        assert_eq!(rows[0].exclusive, 370 - 32 - 52);
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["main.tex", "siunitx", "article", "translations", "size10.clo"]
        );
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    }

    #[test]
    fn repeated_loads_aggregate() {
        let m = model(
            "start 0 0 pdftex\nfile+ 0 0 main.tex\nfile+ 10 1 t1pcr.fd\nfile- 15 1 t1pcr.fd\nfile+ 20 1 t1pcr.fd\nfile- 22 1 t1pcr.fd\nend 30 0 \n",
        );
        let fd = m.unit_table().into_iter().find(|r| r.name == "t1pcr.fd").unwrap();
        assert_eq!((fd.loads, fd.inclusive, fd.exclusive), (2, 7, 7));
    }

    #[test]
    fn errors_name_the_line() {
        let e = BuildModel::from_records(
            &parse_str("start 0 0 pdftex\npkg+ 1 0 a\nfile+ 2 0 a.sty\npkg- 3 0 a\n", "t").unwrap(),
            "t",
        )
        .unwrap_err();
        assert_eq!(e.to_string(), "t:4: pkg- a closes file+ a.sty opened at line 3");
        let e = BuildModel::from_records(&parse_str("file+ 0 0 x\n", "t").unwrap(), "t").unwrap_err();
        assert_eq!(e.to_string(), "t:1: first event is not `start`");
    }

    #[test]
    fn phases_environments_and_discarded_shipouts() {
        let m = model(
            "start 100 0 luatex 2026-11-01
file+ 100 0 main.tex
pkg+ 110 1 tikz
file+ 111 1 tikz.sty
file- 150 1 tikz.sty
pkg- 151 1 tikz
predoc 200 1
begindoc 210 1
docstart 230 1
env+ 240 1 tikzpicture
env+ 245 1 tikzpicture
env- 250 1 tikzpicture
env- 260 1 tikzpicture
env+ 270 1 longtable
ship 280 1 1
page 290 1 1
env- 300 1 longtable
ship 310 1 2
ship 320 1 2
page 325 1 2
enddoc 330 1
ship 340 1 3
page 350 1 3
lastpage 352 1
env+ 353 1 unclosed
end 360 1
",
        );
        let p = m.run_phases();
        assert_eq!(p.startup, 100);
        assert_eq!(p.preamble, Some(100));
        assert_eq!(p.begin_document, Some(30), "predoc to docstart");
        assert_eq!(p.shipout, 10 + 5 + 10, "the discarded ship at 310 is not a shipout");
        assert_eq!(p.typesetting, Some(100 - 15), "docstart..enddoc less two shipouts");
        assert_eq!(p.end_document, Some(30 - 10));
        assert_eq!(p.after_last_page, Some(8));
        assert_eq!(p.finalization, None, "no wall time from records alone");
        let sum = p.startup
            + p.preamble.unwrap()
            + p.begin_document.unwrap()
            + p.typesetting.unwrap()
            + p.shipout
            + p.end_document.unwrap();
        assert_eq!(sum, p.last_record, "the six engine phases add up to the last record");
        assert_eq!(m.discarded_ships, [2]);
        assert_eq!(m.page_times()[0], PageTime { page: 1, time: 60 }, "page 1 counts from docstart");
        let envs = m.env_table();
        let get = |n: &str| envs.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!((get("tikzpicture").count, get("tikzpicture").total), (2, 20), "nested counted, not timed twice");
        assert_eq!(get("longtable").total, 30 - 10, "the shipout inside is left out");
        assert_eq!(get("unclosed").total, 7, "closed at the last record");
        let mut w = m.clone();
        w.wall = Some(400);
        assert_eq!(w.run_phases().finalization, Some(40));
    }

    #[test]
    fn old_records_fall_back_to_begindoc() {
        let m = model(
            "start 0 0 pdftex\nfile+ 0 0 main.tex\nbegindoc 10 1 \nship 15 1 1\npage 20 1 1\nenddoc 25 1 \nend 30 1 \n",
        );
        let p = m.run_phases();
        assert_eq!(
            (p.preamble, p.begin_document, p.typesetting, p.end_document),
            (Some(10), Some(0), Some(10), Some(5))
        );
        assert_eq!(p.after_last_page, None);
        assert_eq!(m.page_times()[0].time, 10);
        assert!(m.envs.is_empty() && m.discarded_ships.is_empty());
    }

    /// The probe on the first line of the main file (Overleaf): no frame for the main file,
    /// the class is the first unit.
    #[test]
    fn records_without_a_main_file_frame() {
        let m = model(
            "start 100 0 pdftex 2026-11-01\ncls+ 101 0 article\nfile+ 102 0 article.cls\nfile- 120 0 article.cls\ncls- 121 0 article\npredoc 150 0 \nbegindoc 151 0 \ndocstart 160 0 \nship 170 0 1\npage 180 0 1\nenddoc 190 0 \nlastpage 191 0 \nend 200 0 \n",
        );
        assert!(m.main_file().is_none());
        assert_eq!(m.root.children[0].name, "article");
        let p = m.run_phases();
        assert_eq!((p.preamble, p.begin_document, p.typesetting), (Some(50), Some(10), Some(20)));
        let text = crate::report::time_text(
            &m,
            "output",
            crate::report::TextOptions {
                files_read: None,
                by: crate::report::TimeBy::Unit,
                top: 12,
                depth: 3,
                look: None,
                no_wall: crate::report::NoWall::Records,
            },
        );
        assert!(text.contains("hint: article is"), "{text}");
        assert!(text.contains("prumo did not run this build"), "{text}");
    }

    #[test]
    fn unclosed_non_tex_frame_is_a_warning() {
        let m = model("start 0 0 pdftex\nfile+ 0 0 main.tex\npkg+ 5 1 broken\nend 9 1 \n");
        assert_eq!(m.warnings, ["line 3: pkg broken never closed"]);
        assert_eq!(m.main_file().unwrap().children[0].end, 9);
    }
}
