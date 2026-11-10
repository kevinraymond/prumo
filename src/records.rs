//! Parser for `<job>.prumo`, the event record written by `tex/prumo.sty`.
//!
//! One event per line: `<event> <timer> <depth> <name>`. The name is the last field and
//! may contain spaces or be empty. See `docs/04-probe.md` for the table of events.

use crate::Scaled;
use std::fmt;
use std::path::Path;

/// The TeX engine that wrote the record, from the `start` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    PdfTex,
    XeTex,
    LuaTex,
    /// An engine name prumo does not know; carried through, never dropped.
    Other,
}

impl Engine {
    /// Parse the engine name as `\c_sys_engine_str` reports it.
    pub fn from_sys_engine(name: &str) -> Engine {
        match name {
            "pdftex" => Engine::PdfTex,
            "xetex" => Engine::XeTex,
            "luatex" | "luahbtex" => Engine::LuaTex,
            _ => Engine::Other,
        }
    }

    /// The `tlmgr` package that ships the engine binary.
    pub fn package(self) -> Option<&'static str> {
        match self {
            Engine::PdfTex => Some("pdftex"),
            Engine::XeTex => Some("xetex"),
            Engine::LuaTex => Some("luatex"),
            Engine::Other => None,
        }
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Engine::PdfTex => "pdftex",
            Engine::XeTex => "xetex",
            Engine::LuaTex => "luatex",
            Engine::Other => "other",
        })
    }
}

/// Which kind of unit a `+`/`-` pair brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitKind {
    File,
    Pkg,
    Cls,
}

impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnitKind::File => "file",
            UnitKind::Pkg => "pkg",
            UnitKind::Cls => "cls",
        })
    }
}

/// One event from the record file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// First event: engine name and `\fmtversion` (kernel date, `YYYY-MM-DD`).
    Start { engine: Engine, engine_name: String, format_date: String },
    /// `file+`, `pkg+`, `cls+`: a unit begins. `name` is the file name as LaTeX saw it
    /// (`t1pcr.fd`) or the package/class name without extension (`siunitx`).
    Open { kind: UnitKind, name: String },
    /// `file-`, `pkg-`, `cls-`: the unit ends.
    Close { kind: UnitKind, name: String },
    /// Raw option list for the package opened by the preceding `pkg+`; may be empty.
    Opts(String),
    /// `\begin{document}` started (`begindocument/before`): the end of the preamble proper.
    /// Packages loaded between this and `begindoc` come from other packages' hooks.
    /// Absent in records from probe versions before 2026-11-06.
    PreDoc,
    /// `\begin{document}` reached (`begindocument`).
    BeginDoc,
    /// `\begin{document}` finished (`begindocument/end`): every `\AtBeginDocument` hook has
    /// run. Page 1 counts from here. Absent before probe v0.2 (2026-11-09).
    DocStart,
    /// A shipout starts (`shipout/before`); `n` is the page about to be shipped. Absent in
    /// records from probe versions before 2026-11-06.
    Ship(u32),
    /// A page was shipped out (`shipout/after`); `n` is prumo's own 1-based counter.
    Page(u32),
    /// `\end{document}` reached.
    EndDoc,
    /// The last page is out (`enddocument/afterlastpage`); what follows is the second
    /// read of the `.aux` and the rerun checks. Absent before probe v0.2.
    LastPage,
    /// `\begin{<name>}` of an environment the run asked to watch (`env/<name>/before`).
    EnvOpen(String),
    /// `\end{<name>}` of a watched environment (`env/<name>/after`).
    EnvClose(String),
    /// Last event; the record file is closed after it.
    End,
}

/// An event with its timestamp and the probe's nesting depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 1-based line in the record file, for error messages.
    pub line: usize,
    /// `\sys_timer:` at the event, scaled seconds since engine start.
    pub time: Scaled,
    /// File nesting depth the probe counted (0 = main-file level). Informational.
    pub depth: i32,
    pub event: Event,
}

/// A parse failure; always names the file and line.
#[derive(Debug, thiserror::Error)]
pub enum RecordsError {
    #[error("{file}:{line}: {msg}")]
    Line { file: String, line: usize, msg: String },
    #[error("{file}: record file is empty")]
    Empty { file: String },
    #[error("{file}: cannot read: {source}")]
    Io {
        file: String,
        #[source]
        source: std::io::Error,
    },
}

/// Parse a record file from disk.
pub fn parse_file(path: &Path) -> Result<Vec<Record>, RecordsError> {
    let file = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|source| RecordsError::Io { file: file.clone(), source })?;
    parse_str(&String::from_utf8_lossy(&bytes), &file)
}

/// Parse record text. `file` is only used in error messages.
pub(crate) fn parse_str(text: &str, file: &str) -> Result<Vec<Record>, RecordsError> {
    let err = |line: usize, msg: String| RecordsError::Line { file: file.to_string(), line, msg };
    let mut out = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            continue;
        }
        // `<event> <timer> <depth> <name>`: name is everything after the third space.
        let mut parts = raw.splitn(4, ' ');
        let ev = parts.next().unwrap_or_default();
        let timer = parts.next().ok_or_else(|| err(line, "missing timer field".into()))?;
        let depth = parts.next().ok_or_else(|| err(line, "missing depth field".into()))?;
        let name = parts.next().unwrap_or_default();
        let time: Scaled =
            timer.parse().map_err(|_| err(line, format!("timer is not a non-negative integer: {timer:?}")))?;
        let depth: i32 = depth.parse().map_err(|_| err(line, format!("depth is not an integer: {depth:?}")))?;
        let event = match ev {
            "start" => {
                let (engine_name, format_date) = name.split_once(' ').unwrap_or((name, ""));
                Event::Start {
                    engine: Engine::from_sys_engine(engine_name),
                    engine_name: engine_name.to_string(),
                    format_date: format_date.to_string(),
                }
            }
            "file+" => Event::Open { kind: UnitKind::File, name: name.to_string() },
            "file-" => Event::Close { kind: UnitKind::File, name: name.to_string() },
            "pkg+" => Event::Open { kind: UnitKind::Pkg, name: name.to_string() },
            "pkg-" => Event::Close { kind: UnitKind::Pkg, name: name.to_string() },
            "cls+" => Event::Open { kind: UnitKind::Cls, name: name.to_string() },
            "cls-" => Event::Close { kind: UnitKind::Cls, name: name.to_string() },
            "opts" => Event::Opts(name.to_string()),
            "predoc" => Event::PreDoc,
            "begindoc" => Event::BeginDoc,
            "docstart" => Event::DocStart,
            "ship" => Event::Ship(
                name.trim().parse().map_err(|_| err(line, format!("page number is not an integer: {name:?}")))?,
            ),
            "page" => Event::Page(
                name.trim().parse().map_err(|_| err(line, format!("page number is not an integer: {name:?}")))?,
            ),
            "enddoc" => Event::EndDoc,
            "lastpage" => Event::LastPage,
            "env+" | "env-" if name.is_empty() => return Err(err(line, format!("{ev} without a name"))),
            "env+" => Event::EnvOpen(name.to_string()),
            "env-" => Event::EnvClose(name.to_string()),
            "end" => Event::End,
            other => return Err(err(line, format!("unknown event {other:?}"))),
        };
        if matches!(event, Event::Open { .. } | Event::Close { .. }) && name.is_empty() {
            return Err(err(line, format!("{ev} without a name")));
        }
        out.push(Record { line, time, depth, event });
    }
    if out.is_empty() {
        return Err(RecordsError::Empty { file: file.to_string() });
    }
    Ok(out)
}

/// Structural checks beyond line syntax: first event is `start`, timestamps never go
/// backwards, every `-` matches the innermost open `+`. Returns the line of the offender.
/// Environment records are not part of the file nesting (an environment can open in one
/// file and close in another), so they are not checked here; the model pairs them.
pub fn validate(records: &[Record], file: &str) -> Result<(), RecordsError> {
    let err = |line: usize, msg: String| RecordsError::Line { file: file.to_string(), line, msg };
    match records.first() {
        Some(Record { event: Event::Start { .. }, .. }) => {}
        Some(r) => return Err(err(r.line, "first event is not `start`".into())),
        None => return Err(RecordsError::Empty { file: file.to_string() }),
    }
    let mut stack: Vec<(UnitKind, &str, usize)> = Vec::new();
    let mut last = 0;
    for r in records {
        if r.time < last {
            return Err(err(r.line, format!("timer went backwards ({} after {last})", r.time)));
        }
        last = r.time;
        match &r.event {
            Event::Open { kind, name } => stack.push((*kind, name, r.line)),
            Event::Close { kind, name } => match stack.pop() {
                Some((k, n, _)) if k == *kind && n == name => {}
                Some((k, n, l)) => {
                    return Err(err(r.line, format!("{kind}- {name} closes {k}+ {n} opened at line {l}")));
                }
                None => return Err(err(r.line, format!("{kind}- {name} with nothing open"))),
            },
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "start 5450 0 pdftex 2023-11-01
file+ 5450 0 main.tex
cls+ 5513 1 article
file+ 5550 1 article.cls
file- 5780 1 article.cls
cls- 5804 1 article
pkg+ 5863 1 fontenc
opts 5864 1 T1
pkg- 5980 1 fontenc
pkg+ 6039 1 amsmath
opts 6039 1
pkg- 6100 1 amsmath
file+ 7000 1 My Dir/odd name.tex
file- 7100 1 My Dir/odd name.tex
begindoc 16471 1
page 18248 1 1
enddoc 18389 1
page 18442 1 2
end 18492 1
";

    #[test]
    fn parses_every_event_kind() {
        let r = parse_str(SAMPLE, "t.prumo").unwrap();
        assert_eq!(r.len(), 19);
        assert_eq!(
            r[0].event,
            Event::Start { engine: Engine::PdfTex, engine_name: "pdftex".into(), format_date: "2023-11-01".into() }
        );
        assert_eq!(r[1].event, Event::Open { kind: UnitKind::File, name: "main.tex".into() });
        assert_eq!(r[1].time, 5450);
        assert_eq!(r[1].depth, 0);
        assert_eq!(r[7].event, Event::Opts("T1".into()));
        assert_eq!(r[10].event, Event::Opts(String::new()), "empty option list keeps the trailing space");
        assert_eq!(r[12].event, Event::Open { kind: UnitKind::File, name: "My Dir/odd name.tex".into() });
        assert_eq!(r[14].event, Event::BeginDoc);
        assert_eq!(r[15].event, Event::Page(1));
        assert_eq!(r[17].event, Event::Page(2));
        assert_eq!(r[18].event, Event::End);
        validate(&r, "t.prumo").unwrap();
    }

    #[test]
    fn probe_v02_events() {
        let r = parse_str(
            "start 1 0 luatex 2026-11-01\npredoc 2 1 \nbegindoc 3 1 \ndocstart 4 1 \nenv+ 5 1 align*\nenv- 6 1 align*\nship 7 1 1\nship 8 1 1\npage 9 1 1\nenddoc 10 1 \nlastpage 11 1 \nend 12 1 \n",
            "t",
        )
        .unwrap();
        assert_eq!(r[3].event, Event::DocStart);
        assert_eq!(r[4].event, Event::EnvOpen("align*".into()));
        assert_eq!(r[5].event, Event::EnvClose("align*".into()));
        assert_eq!(r[10].event, Event::LastPage);
        validate(&r, "t").unwrap();
        let e = parse_str("start 1 0 pdftex\nenv+ 2 0 \n", "j").unwrap_err();
        assert_eq!(e.to_string(), "j:2: env+ without a name");
    }

    #[test]
    fn engine_names() {
        assert_eq!(Engine::from_sys_engine("luahbtex"), Engine::LuaTex);
        assert_eq!(Engine::from_sys_engine("xetex"), Engine::XeTex);
        assert_eq!(Engine::from_sys_engine("ptex"), Engine::Other);
    }

    #[test]
    fn errors_name_file_and_line() {
        let e = parse_str("start 1 0 pdftex 2023-11-01\nbogus 2 0 x\n", "j.prumo").unwrap_err();
        assert_eq!(e.to_string(), "j.prumo:2: unknown event \"bogus\"");
        let e = parse_str("start x 0 pdftex\n", "j.prumo").unwrap_err();
        assert!(e.to_string().starts_with("j.prumo:1: timer is not"));
        let e = parse_str("start 1 0 pdftex\nfile+ 2 0 \n", "j.prumo").unwrap_err();
        assert_eq!(e.to_string(), "j.prumo:2: file+ without a name");
        assert!(matches!(parse_str("\n\n", "e.prumo"), Err(RecordsError::Empty { .. })));
    }

    #[test]
    fn validate_catches_mismatch_and_time_travel() {
        let r = parse_str("start 1 0 pdftex\npkg+ 2 0 a\nfile+ 3 0 a.sty\npkg- 4 0 a\n", "j").unwrap();
        let e = validate(&r, "j").unwrap_err().to_string();
        assert_eq!(e, "j:4: pkg- a closes file+ a.sty opened at line 3");
        let r = parse_str("start 9 0 pdftex\nfile+ 8 0 a\n", "j").unwrap();
        assert!(validate(&r, "j").unwrap_err().to_string().contains("timer went backwards"));
        let r = parse_str("file+ 1 0 a\n", "j").unwrap();
        assert!(validate(&r, "j").unwrap_err().to_string().contains("not `start`"));
        // An unclosed main.tex is legal: \end{document} never returns from \input.
        let r = parse_str("start 1 0 pdftex\nfile+ 2 0 main.tex\nend 3 0 \n", "j").unwrap();
        validate(&r, "j").unwrap();
    }
}
