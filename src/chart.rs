//! Terminal charts for `prumo time`: the HTML page's bars, timeline and page strip drawn
//! with Unicode block elements. No dependency, no full-screen UI. Used only when stdout is
//! a UTF-8 terminal and `--plain` is not given; pipes get the plain text.

use crate::Scaled;
use crate::model::{BuildModel, Frame, FrameKind, UnitRow};
use crate::report::ms;
use std::fmt::Write as _;
use std::io::IsTerminal;

/// How to draw: the terminal's width and whether ANSI color is welcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub width: usize,
    pub color: bool,
    /// Expand the preamble in the timeline (`--preamble`).
    pub preamble: bool,
}

/// Charts or not, from the environment: a terminal, a UTF-8 locale, not `TERM=dumb`,
/// not `--plain`. `NO_COLOR` keeps the charts and drops the color.
pub fn detect(plain: bool) -> Option<Look> {
    if plain || !std::io::stdout().is_terminal() {
        return None;
    }
    detect_with(|k| std::env::var(k).ok(), tput_cols)
}

fn detect_with(var: impl Fn(&str) -> Option<String>, cols: impl Fn() -> Option<usize>) -> Option<Look> {
    if var("TERM").is_some_and(|t| t == "dumb") {
        return None;
    }
    let utf8 = cfg!(windows)
        || ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|k| var(k).filter(|v| !v.is_empty()))
            .is_some_and(|v| v.to_ascii_lowercase().contains("utf"));
    if !utf8 {
        return None;
    }
    let width = var("COLUMNS").and_then(|c| c.trim().parse().ok()).or_else(cols).unwrap_or(100).clamp(60, 160);
    let color = var("NO_COLOR").is_none_or(|v| v.is_empty());
    Some(Look { width, color, preamble: false })
}

fn tput_cols() -> Option<usize> {
    let out = std::process::Command::new("tput").arg("cols").output().ok()?;
    String::from_utf8(out.stdout).ok()?.trim().parse().ok()
}

const EIGHTHS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A left-anchored bar of `frac` (0..=1) of `cells`, in eighths; never empty for `frac > 0`.
pub(crate) fn bar(frac: f64, cells: usize) -> String {
    let eighths = ((frac.clamp(0.0, 1.0) * cells as f64 * 8.0).round() as usize).max(usize::from(frac > 0.0));
    let full = eighths / 8;
    let rest = eighths % 8;
    let mut s: String = "█".repeat(full.min(cells));
    if rest > 0 && full < cells {
        s.push(EIGHTHS[rest]);
    }
    s
}

fn paint(kind: FrameKind, text: &str, color: bool) -> String {
    if !color {
        return text.to_string();
    }
    let code = match kind {
        FrameKind::Cls => "34",
        FrameKind::Pkg => "32",
        FrameKind::File => "33",
        FrameKind::Job => "90",
    };
    format!("\x1b[{code}m{text}\x1b[0m")
}

fn dim(text: &str, color: bool) -> String {
    if color { format!("\x1b[2m{text}\x1b[0m") } else { text.to_string() }
}

/// The unit table with a bar column: exclusive time, scaled to the slowest row.
pub fn unit_table(rows: &[&UnitRow], top: usize, look: Look) -> String {
    let mut s = String::new();
    let shown: Vec<&&UnitRow> = rows.iter().take(top).collect();
    let Some(max) = shown.iter().map(|r| r.exclusive).max().filter(|m| *m > 0) else {
        return s;
    };
    let name_w = shown.iter().map(|r| r.name.chars().count()).max().unwrap_or(4).min(40);
    // "  excl ms  incl ms  kind  name  bar"
    let cells = look.width.saturating_sub(10 + 10 + 6 + name_w + 4).clamp(10, 60);
    for r in shown {
        let name: String = r.name.chars().take(name_w).collect();
        let _ = writeln!(
            s,
            "  {:>7.1} ms {:>7.1} incl  {} {:<name_w$}  {}",
            ms(r.exclusive),
            ms(r.inclusive),
            paint(r.kind, &format!("{:<4}", r.kind.to_string()), look.color),
            name,
            paint(r.kind, &bar(r.exclusive as f64 / max as f64, cells), look.color)
        );
    }
    s
}

/// A tick step in ms from {1, 2, 5} × 10^k giving at least four ticks across `total_ms`.
fn nice_step(total_ms: f64) -> f64 {
    let mut step = 1.0;
    loop {
        for m in [1.0, 2.0, 5.0] {
            let cand = step * m;
            if total_ms / cand <= 8.0 {
                return cand;
            }
        }
        step *= 10.0;
    }
}

fn fmt_tick(ms: f64) -> String {
    if ms >= 1000.0 { format!("{:.1} s", ms / 1000.0) } else { format!("{ms:.0} ms") }
}

/// One row of the timeline: a frame, or the collapsed preamble.
struct Row<'a> {
    kind: FrameKind,
    label: String,
    start: Scaled,
    end: Scaled,
    frame: Option<&'a Frame>,
}

/// The rows of the timeline: with the preamble expanded, every frame down to `depth` that
/// is worth a cell; otherwise the main file and the folded preamble.
fn rows<'a>(m: &'a BuildModel, depth: usize, preamble: bool, begindoc: Scaled, threshold: Scaled) -> Vec<Row<'a>> {
    let mut rows: Vec<Row> = Vec::new();
    if preamble {
        m.root.walk(&mut |fr, d| {
            if d <= depth && fr.start < begindoc && fr.end.min(begindoc).saturating_sub(fr.start) >= threshold {
                rows.push(Row {
                    kind: fr.kind,
                    label: format!("{}{}", "  ".repeat(d), fr.name),
                    start: fr.start,
                    end: fr.end.min(begindoc),
                    frame: Some(fr),
                });
            }
        });
    } else {
        if let Some(main) = m.main_file() {
            rows.push(Row {
                kind: main.kind,
                label: main.name.clone(),
                start: main.start,
                end: main.end,
                frame: Some(main),
            });
        }
        rows.push(Row {
            kind: FrameKind::Job,
            label: format!("  preamble ({} units)", m.preamble_units()),
            start: m.start,
            end: begindoc,
            frame: None,
        });
    }
    rows
}

/// The axis: tick labels on one line, the line with a `┬` under each label.
fn axis_lines(total_ms: f64, cells: usize, label_w: usize, color: bool) -> String {
    let step = nice_step(total_ms);
    let mut axis = vec![' '; cells + 8];
    let mut line = vec!['─'; cells];
    let mut t = 0.0;
    while t <= total_ms {
        let c = (t / total_ms * cells as f64) as usize;
        if c < cells {
            line[c] = '┬';
            for (i, ch) in fmt_tick(t).chars().enumerate() {
                if c + i < axis.len() {
                    axis[c + i] = ch;
                }
            }
        }
        t += step;
    }
    let mut s = String::new();
    let _ = writeln!(s, "  {:label_w$}  {}", "", dim(axis.iter().collect::<String>().trim_end(), color));
    let _ = writeln!(s, "  {:label_w$}  {}", "", dim(&line.iter().collect::<String>(), color));
    s
}

/// One bar from column `start` to `end`: full cells, the last one trimmed to eighths so a
/// short frame reads as short, and a `┆` in every empty cell a phase boundary falls on.
fn row_cells(start: f64, end: f64, cells: usize, phases: &[usize]) -> String {
    let end = end.max(start + 0.125);
    let mut cellrow: Vec<char> = vec![' '; cells];
    let first = (start.floor() as usize).min(cells - 1);
    let last = (end.ceil() as usize).min(cells);
    for c in cellrow.iter_mut().take(last).skip(first) {
        *c = '█';
    }
    if last > first && last <= cells {
        let frac = end - (last - 1) as f64;
        if frac < 1.0 && last > first {
            cellrow[last - 1] = EIGHTHS[((frac * 8.0).round() as usize).clamp(1, 8)];
        }
    }
    for &p in phases {
        if cellrow[p] == ' ' {
            cellrow[p] = '┆';
        }
    }
    cellrow.iter().collect()
}

/// The pages row: for each cell, is the time mostly typesetting (`░`) or mostly shipout
/// hooks (`▓`)?
fn pages_strip(m: &BuildModel, cells: usize, total: Scaled, col: &dyn Fn(Scaled) -> f64) -> String {
    let per_cell = total as f64 / cells as f64;
    let mut typeset = vec![0.0f64; cells];
    let mut ship = vec![0.0f64; cells];
    let add = |acc: &mut Vec<f64>, a: Scaled, b: Scaled| {
        if b <= a {
            return;
        }
        let (a, b) = (col(a), col(b));
        let (c0, c1) = (a.floor() as usize, (b.ceil() as usize).min(cells));
        for (c, slot) in acc.iter_mut().enumerate().take(c1).skip(c0) {
            let lo = a.max(c as f64);
            let hi = b.min(c as f64 + 1.0);
            *slot += (hi - lo).max(0.0) * per_cell;
        }
    };
    let mut prev = m.body_start();
    for p in &m.pages {
        if let Some(t0) = prev {
            match p.ship_start {
                Some(sb) => {
                    add(&mut typeset, t0, sb);
                    add(&mut ship, sb, p.time);
                }
                None => add(&mut typeset, t0, p.time),
            }
        }
        prev = Some(p.time);
    }
    (0..cells)
        .map(|c| {
            if typeset[c] + ship[c] <= 0.0 {
                ' '
            } else if ship[c] > typeset[c] {
                '▓'
            } else {
                '░'
            }
        })
        .collect()
}

/// The load tree over time. Collapsed (the default): the main file, one row for the whole
/// preamble, and the pages row (`░` typesetting, `▓` shipout hooks) on an axis from the
/// engine start to the last record; the preamble is the same every build, so it stays
/// folded. `preamble`: the axis ends at `\begin{document}` and every frame down to
/// `depth` that is worth a cell gets a row. Phase boundaries are `┆`.
pub fn timeline(m: &BuildModel, depth: usize, look: Look) -> String {
    const MAX_ROWS: usize = 40;
    let preamble = look.preamble;
    let begindoc = m.phases.begindoc.unwrap_or(m.end);
    let axis_end = if preamble { begindoc } else { m.end };
    let total = (axis_end - m.start).max(1);
    // A quarter of a percent: below that a bar is a sliver anyway.
    let mut rows = rows(m, depth, preamble, begindoc, total / 400);
    let hidden = rows.len().saturating_sub(MAX_ROWS);
    rows.truncate(MAX_ROWS);
    let label_w = rows.iter().map(|r| r.label.chars().count()).max().unwrap_or(10).clamp(10, 30);
    let cells = look.width.saturating_sub(label_w + 2 + 10).clamp(20, 120);
    let col = |t: Scaled| -> f64 { (t.saturating_sub(m.start).min(total)) as f64 / total as f64 * cells as f64 };

    let mut s = axis_lines(ms(total), cells, label_w, look.color);
    let phases: Vec<usize> = [m.phases.predoc.or(m.phases.begindoc), m.phases.enddoc]
        .into_iter()
        .flatten()
        .filter(|t| *t < axis_end)
        .map(|t| (col(t) as usize).min(cells - 1))
        .collect();
    for r in &rows {
        let bar = row_cells(col(r.start), col(r.end), cells, &phases);
        let label: String = r.label.chars().take(label_w).collect();
        let label = format!("{label:<label_w$}");
        let took = match r.frame {
            Some(fr) => fr.inclusive().min(r.end.saturating_sub(r.start)),
            None => r.end.saturating_sub(r.start),
        };
        let _ = writeln!(
            s,
            "  {}  {}{:>7.0} ms",
            paint(r.kind, &label, look.color),
            paint(r.kind, &bar, look.color),
            ms(took)
        );
    }
    if hidden > 0 {
        let _ = writeln!(s, "  {:<label_w$}  … {hidden} more rows (--depth, or --html)", "");
    }
    if !m.pages.is_empty() && !preamble {
        let strip = pages_strip(m, cells, total, &col);
        let painted = if look.color { format!("\x1b[34m{strip}\x1b[0m") } else { strip };
        let _ = writeln!(s, "  {:<label_w$}  {painted}{:>5} pages", "pages", m.pages.len());
    }
    let legend = if preamble {
        "x: time since engine start, to \\begin{document}; rows: nesting depth; ┆ begindocument/before"
    } else {
        "x: time since engine start; ┆ end of preamble / \\end{document}; pages: ░ typesetting ▓ shipout hooks; --preamble expands the preamble on its own axis"
    };
    let _ = writeln!(s, "  {}", dim(legend, look.color));
    s
}

/// One cell per page, height by that page's time, in rows of `width` pages with the first
/// page number of each row as its label.
pub(crate) fn page_spark(m: &BuildModel, look: Look) -> String {
    let times = m.page_times();
    let Some(max) = times.iter().map(|p| p.time).max().filter(|m| *m > 0) else {
        return String::new();
    };
    let per_row = look.width.saturating_sub(12).clamp(20, 120);
    let mut s = String::new();
    for chunk in times.chunks(per_row) {
        let spark: String =
            chunk.iter().map(|p| SPARK[(((p.time as f64 / max as f64) * 7.0).round() as usize).min(7)]).collect();
        let _ = writeln!(s, "  {:>6}  {}", format!("p{}", chunk[0].page), spark);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::parse_str;

    fn model() -> BuildModel {
        let recs = parse_str(
            "start 0 0 pdftex\nfile+ 0 0 main.tex\ncls+ 10 1 article\nfile+ 10 1 article.cls\nfile- 30 1 article.cls\ncls- 30 1 article\npkg+ 30 1 siunitx\nopts 30 1 \nfile+ 31 1 siunitx.sty\nfile- 80 1 siunitx.sty\npkg- 80 1 siunitx\npredoc 100 1 \nbegindoc 120 1 \nship 180 1 1\npage 200 1 1\nship 230 1 2\npage 240 1 2\nenddoc 250 1 \nfile- 255 0 main.tex\nend 256 0 \n",
            "t",
        )
        .unwrap();
        BuildModel::from_records(&recs, "t").unwrap()
    }

    #[test]
    fn bars_are_eighths_and_never_vanish() {
        assert_eq!(bar(1.0, 4), "████");
        assert_eq!(bar(0.5, 4), "██");
        assert_eq!(bar(0.5625, 4), "██▎", "2.25 cells: two full and two eighths");
        assert_eq!(bar(0.0, 4), "");
        assert_eq!(bar(0.001, 4), "▏", "a nonzero value shows");
        assert_eq!(bar(2.0, 3), "███", "clamped");
    }

    #[test]
    fn timeline_rows_align_and_mark_phases() {
        let m = model();
        let look = Look { width: 80, color: false, preamble: false };
        // Collapsed: axis label row, axis line, main.tex, the preamble row, pages, legend.
        let t = timeline(&m, 3, look);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 6, "{t}");
        assert!(lines[2].trim_start().starts_with("main.tex") && lines[2].contains("█"), "{t}");
        assert!(lines[3].trim_start().starts_with("preamble (2 units)") && lines[3].contains("┆"), "{t}");
        assert!(!t.contains("siunitx"), "folded: {t}");
        assert!(lines[4].contains("░") && lines[4].contains("▓") && lines[4].ends_with("2 pages"), "{t}");
        let widths: Vec<usize> = lines[2..4].iter().map(|l| l.chars().count()).collect();
        assert!(widths.iter().all(|w| *w == widths[0]), "rows align: {widths:?}\n{t}");
        assert!(lines[0].contains("0 ms") && lines[1].contains("┬"), "{t}");
        // Color wraps the same text in escapes and nothing else.
        let c = timeline(&m, 3, Look { width: 80, color: true, preamble: false });
        assert_eq!(strip_ansi(&c), t);
        // Expanded: the axis ends at \begin{document}, every frame has a row, no pages row.
        let p = timeline(&m, 3, Look { preamble: true, ..look });
        let lines: Vec<&str> = p.lines().collect();
        assert_eq!(lines.len(), 6, "{p}");
        assert!(
            lines[2].trim_start().starts_with("main.tex") && lines[2].trim_end().ends_with(" 2 ms"),
            "clipped to the preamble (120 scaled = 1.8 ms): {p}"
        );
        assert!(lines[3].trim_start().starts_with("article") && lines[4].trim_start().starts_with("siunitx"), "{p}");
        assert!(lines[3].contains("┆"), "predoc mark: {p}");
        assert!(!p.contains("░"), "no pages row in the zoom: {p}");
    }

    #[test]
    fn unit_table_scales_to_the_slowest_and_sparkline_counts_pages() {
        let m = model();
        let rows = m.unit_table();
        let refs: Vec<&UnitRow> = rows.iter().collect();
        let t = unit_table(&refs, 10, Look { width: 80, color: false, preamble: false });
        let first = t.lines().next().unwrap();
        assert!(first.contains("siunitx") || first.contains("main.tex"), "{t}");
        let bars: Vec<usize> = t.lines().map(|l| l.chars().filter(|c| *c == '█').count()).collect();
        assert!(bars[0] >= bars[1], "{t}");
        let sp = page_spark(&m, Look { width: 80, color: false, preamble: false });
        assert_eq!(sp.lines().count(), 1);
        assert_eq!(sp.split_whitespace().nth(1).unwrap().chars().count(), 2, "{sp}");
        assert!(sp.contains('█'), "the slowest page is full height: {sp}");
    }

    #[test]
    fn detection_follows_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        let cols = || Some(132);
        assert_eq!(
            detect_with(env(&[("LANG", "en_US.UTF-8")]), cols),
            Some(Look { width: 132, color: true, preamble: false })
        );
        assert_eq!(detect_with(env(&[("LANG", "en_US.UTF-8"), ("NO_COLOR", "1")]), cols).map(|l| l.color), Some(false));
        assert_eq!(detect_with(env(&[("LANG", "en_US.UTF-8"), ("COLUMNS", "70")]), cols).map(|l| l.width), Some(70));
        assert_eq!(detect_with(env(&[("LANG", "en_US.UTF-8"), ("TERM", "dumb")]), cols), None);
        if !cfg!(windows) {
            assert_eq!(detect_with(env(&[("LANG", "C")]), cols), None);
            assert!(detect_with(env(&[("LC_ALL", "pt_BR.utf8"), ("LANG", "C")]), cols).is_some());
        }
        assert_eq!(detect_with(env(&[("LANG", "en_US.UTF-8")]), || None).map(|l| l.width), Some(100));
        assert_eq!(nice_step(1500.0), 200.0);
        assert_eq!(nice_step(250.0), 50.0);
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for d in chars.by_ref() {
                    if d == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
