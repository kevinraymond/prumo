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
//!    since `begindoc` for the first.

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
    pub enddoc: Option<Scaled>,
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
                Event::EndDoc => phases.enddoc = Some(r.time),
                Event::Ship(n) => pending_ship = Some((*n, r.time)),
                Event::Page(n) => {
                    let ship_start = pending_ship.take().filter(|(m, _)| m == n).map(|(_, t)| t);
                    pages.push(Page { number: *n, time: r.time, ship_start });
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
            warnings,
        })
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

    /// Rule 5: per-page time. The first page counts from `begindoc` (0 if there was none,
    /// as in the oracle).
    pub fn page_times(&self) -> Vec<(u32, Scaled)> {
        let mut prev = self.phases.begindoc;
        self.pages
            .iter()
            .map(|p| {
                let dt = prev.map_or(0, |t| p.time.saturating_sub(t));
                prev = Some(p.time);
                (p.number, dt)
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

    /// Per page: (number, typesetting time, shipout time). Shipout time is `None` when the
    /// records have no `ship` events (older probe).
    pub fn page_split(&self) -> Vec<(u32, Scaled, Option<Scaled>)> {
        let mut prev = self.phases.begindoc;
        self.pages
            .iter()
            .map(|p| {
                let total = prev.map_or(0, |t| p.time.saturating_sub(t));
                prev = Some(p.time);
                match p.ship_start {
                    Some(s) => {
                        (p.number, total.saturating_sub(p.time.saturating_sub(s)), Some(p.time.saturating_sub(s)))
                    }
                    None => (p.number, total, None),
                }
            })
            .collect()
    }

    /// The main file frame: the first `.tex` child of the job.
    pub fn main_file(&self) -> Option<&Frame> {
        self.root.children.iter().find(|f| f.kind == FrameKind::File)
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
        assert_eq!(m.page_times(), vec![(1, 100), (2, 60)], "page 2 counts from page 1, not enddoc");
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
    fn unclosed_non_tex_frame_is_a_warning() {
        let m = model("start 0 0 pdftex\nfile+ 0 0 main.tex\npkg+ 5 1 broken\nend 9 1 \n");
        assert_eq!(m.warnings, ["line 3: pkg broken never closed"]);
        assert_eq!(m.main_file().unwrap().children[0].end, 9);
    }
}
