//! `prumo time --html`: one self-contained page (inline CSS and a few lines of JS, no
//! network, no external files) with a flame chart of the load tree over time (the preamble
//! folded into one row, expanded on its own axis under it), the page strip (typesetting vs
//! shipout), the unit table, the tree, and the fonts report.
//!
//! The layout is computed here in Rust and emitted as SVG, so the page reads without
//! JavaScript; JS only switches tabs and keeps the chart's hover label in place.

use crate::Scaled;
use crate::fonts::{FontReport, Status};
use crate::model::{BuildModel, Frame, FrameKind};
use crate::report::ms;
use std::fmt::Write as _;

/// Escape text for an HTML or SVG text node or attribute value.
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const CHART_W: f64 = 1200.0;
const ROW_H: f64 = 22.0;
/// Approximate advance of one character at the chart's font size, in viewBox units.
const CHAR_W: f64 = 6.4;

fn kind_class(k: FrameKind) -> &'static str {
    match k {
        FrameKind::Job => "job",
        FrameKind::Cls => "cls",
        FrameKind::Pkg => "pkg",
        FrameKind::File => "file",
    }
}

fn kind_word(k: FrameKind) -> &'static str {
    match k {
        FrameKind::Job => "job",
        FrameKind::Cls => "class",
        FrameKind::Pkg => "package",
        FrameKind::File => "file",
    }
}

/// Fit a label into `width` viewBox units, or nothing.
fn fit_label(name: &str, width: f64) -> Option<String> {
    let chars = ((width - 6.0) / CHAR_W).floor() as usize;
    let n = name.chars().count();
    if chars >= n {
        Some(name.to_string())
    } else if chars >= 4 {
        Some(format!("{}…", name.chars().take(chars - 1).collect::<String>()))
    } else {
        None
    }
}

/// Which part of the build a flame chart shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    /// The whole build: the main file, the preamble folded into one row (it is the same
    /// every build), the files read in the body at their depth, and the pages strip.
    Folded,
    /// The preamble on its own axis, engine start to `\begin{document}`, every frame.
    Preamble,
}

/// The flame chart: one row per nesting depth, x is time since engine start.
fn flame_svg(m: &BuildModel, view: View) -> String {
    let begindoc = m.phases.begindoc.unwrap_or(m.end);
    let axis_end = if view == View::Preamble { begindoc } else { m.end };
    let total = (axis_end - m.start).max(1) as f64;
    let x_of = |t: Scaled| (t.saturating_sub(m.start).min(axis_end - m.start)) as f64 / total * CHART_W;
    // A frame is drawn in the preamble view when it started before `\begin{document}`
    // (clipped there), and in the folded view when it is the main file or started after.
    let shown = |fr: &Frame, depth: usize| match view {
        View::Preamble => fr.start < begindoc,
        View::Folded => depth == 0 || fr.start >= begindoc,
    };
    // `walk` visits the job's children at depth 0, so the main file is row 0.
    let mut depth_max = 0;
    m.root.walk(&mut |fr, d| {
        if shown(fr, d) {
            depth_max = depth_max.max(d);
        }
    });
    if view == View::Folded {
        depth_max = depth_max.max(1); // the preamble row
    }
    let rows = depth_max + 1;
    let pages = view == View::Folded && !m.pages.is_empty();
    let pages_h = if pages { ROW_H + 6.0 } else { 0.0 };
    let axis_h = 18.0;
    let h = rows as f64 * ROW_H + pages_h + axis_h + 4.0;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg class="flame" viewBox="0 0 {CHART_W} {h}" width="100%" role="img" aria-label="{}">"#,
        if view == View::Preamble { "preamble over time" } else { "load tree over time" }
    );
    // Axis: ticks every nice step.
    let total_ms = ms(axis_end - m.start);
    let step = nice_step(total_ms / 8.0);
    let mut t = 0.0;
    while t <= total_ms + 1e-9 {
        let x = t / total_ms.max(1e-9) * CHART_W;
        if x > CHART_W - 48.0 {
            break;
        }
        let _ = write!(
            s,
            r#"<line class="tick" x1="{x:.1}" y1="0" x2="{x:.1}" y2="{:.1}"/><text class="axis" x="{:.1}" y="{:.1}">{}</text>"#,
            h - axis_h,
            x + 3.0,
            h - 5.0,
            fmt_ms_short(t)
        );
        t += step;
    }
    // Phase lines.
    for (label, at) in [("preamble ends", m.phases.predoc.or(m.phases.begindoc)), ("\\end{document}", m.phases.enddoc)]
    {
        if let Some(at) = at.filter(|at| *at < axis_end) {
            let x = x_of(at);
            let _ = write!(
                s,
                r#"<line class="phase" x1="{x:.1}" y1="0" x2="{x:.1}" y2="{:.1}"><title>{label}</title></line>"#,
                h - axis_h
            );
        }
    }
    // Frames.
    m.root.walk(&mut |fr, depth| {
        if !shown(fr, depth) {
            return;
        }
        let x = x_of(fr.start);
        let w = (x_of(fr.end) - x).max(1.0);
        let y = depth as f64 * ROW_H;
        let opts = fr.options.as_deref().filter(|o| !o.is_empty()).map(|o| format!(" [{o}]")).unwrap_or_default();
        let _ = write!(
            s,
            r#"<g class="f {}"><rect x="{x:.2}" y="{y:.1}" width="{w:.2}" height="{:.1}"/><title>{} {}{}
{:.1} ms inclusive, {:.1} ms exclusive, depth {depth}</title>"#,
            kind_class(fr.kind),
            ROW_H - 1.0,
            kind_word(fr.kind),
            esc(&fr.name),
            esc(&opts),
            ms(fr.inclusive()),
            ms(fr.exclusive())
        );
        if let Some(label) = fit_label(&fr.name, w) {
            let _ = write!(s, r#"<text x="{:.2}" y="{:.1}">{}</text>"#, x + 3.0, y + ROW_H - 7.0, esc(&label));
        }
        s.push_str("</g>");
    });
    // The folded preamble: one row standing for every unit loaded before `\begin{document}`.
    if view == View::Folded {
        let units = m.preamble_units();
        let x = x_of(m.start);
        let w = (x_of(begindoc) - x).max(1.0);
        let y = ROW_H;
        let label = format!("preamble ({units} units)");
        let _ = write!(
            s,
            r#"<g class="f job fold" role="button" tabindex="0" aria-controls="preamble" aria-expanded="false"><rect x="{x:.2}" y="{y:.1}" width="{w:.2}" height="{:.1}"/><title>preamble: {units} classes and packages, {:.1} ms
click to expand it on its own axis</title>"#,
            ROW_H - 1.0,
            ms(begindoc - m.start)
        );
        if let Some(label) = fit_label(&label, w) {
            let _ = write!(s, r#"<text x="{:.2}" y="{:.1}">{}</text>"#, x + 3.0, y + ROW_H - 7.0, esc(&label));
        }
        s.push_str("</g>");
    }
    // Pages strip: typesetting (light) then shipout (dark) per page.
    if pages {
        let y = rows as f64 * ROW_H + 4.0;
        let mut prev = m.phases.begindoc.unwrap_or(m.start);
        for p in &m.pages {
            let ship = p.ship_start.unwrap_or(p.time);
            let x0 = x_of(prev);
            let x1 = x_of(ship);
            let x2 = x_of(p.time);
            let _ = write!(
                s,
                r#"<g class="page"><rect class="typeset" x="{x0:.2}" y="{y:.1}" width="{:.2}" height="{:.1}"/><rect class="ship" x="{x1:.2}" y="{y:.1}" width="{:.2}" height="{:.1}"/><title>page {}: {:.1} ms typesetting, {:.1} ms shipout</title></g>"#,
                (x1 - x0).max(0.5),
                ROW_H - 1.0,
                (x2 - x1).max(0.5),
                ROW_H - 1.0,
                p.number,
                ms(ship.saturating_sub(prev)),
                ms(p.time.saturating_sub(ship))
            );
            prev = p.time;
        }
        let _ = write!(s, r#"<text class="axis" x="3" y="{:.1}">pages</text>"#, y + ROW_H - 7.0);
    }
    s.push_str("</svg>");
    s
}

fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let r = raw / mag;
    let f = if r < 1.5 {
        1.0
    } else if r < 3.5 {
        2.0
    } else if r < 7.5 {
        5.0
    } else {
        10.0
    };
    f * mag
}

fn fmt_ms_short(t: f64) -> String {
    if t >= 1000.0 { format!("{:.1} s", t / 1000.0) } else { format!("{t:.0} ms") }
}

fn tree_html(fr: &Frame, depth: usize, s: &mut String) {
    let opts = fr.options.as_deref().filter(|o| !o.is_empty()).map(|o| format!(" [{o}]")).unwrap_or_default();
    let open = if depth < 2 { r#" open="""# } else { "" };
    let label = format!(
        r#"<span class="k {}">{}</span> {}{} <span class="t">{:.1} ms incl, {:.1} ms excl</span>"#,
        kind_class(fr.kind),
        kind_word(fr.kind),
        esc(&fr.name),
        esc(&opts),
        ms(fr.inclusive()),
        ms(fr.exclusive())
    );
    if fr.children.is_empty() {
        let _ = write!(s, r#"<li>{label}</li>"#);
    } else {
        let _ = write!(s, r#"<li><details{open}><summary>{label}</summary><ul>"#);
        for c in &fr.children {
            tree_html(c, depth + 1, s);
        }
        s.push_str("</ul></details></li>");
    }
}

fn fonts_html(f: &FontReport) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<p>TFMs loaded: {}, embedded in PDF: {}, Type 3: {}, bitmap .pk read: {}.</p>",
        f.loaded.len(),
        f.embedded_in_pdf.len(),
        f.type3,
        f.bitmap_pk.len()
    );
    for w in &f.warnings {
        let _ = write!(s, r#"<p class="warn">{}</p>"#, esc(w));
    }
    if !f.loaded.is_empty() {
        s.push_str("<h3>Loaded (TFM)</h3><table><tr><th>TFM</th><th>PostScript name</th><th>status</th><th>declared by</th></tr>");
        for l in &f.loaded {
            let status = match l.status {
                Status::Embedded => "embedded",
                Status::NotEmbedded => "loaded, never used",
                Status::Virtual => "virtual font",
                Status::NoMapEntry => "no map entry",
            };
            let _ = write!(
                s,
                "<tr><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                esc(&l.tfm),
                esc(l.ps_name.as_deref().unwrap_or("")),
                if l.status == Status::NotEmbedded { "unused" } else { "" },
                status,
                esc(l.declared_by.as_deref().unwrap_or(""))
            );
        }
        s.push_str("</table>");
    }
    if !f.opentype.is_empty() {
        s.push_str("<h3>OpenType, opened by the engine</h3><table><tr><th>font</th><th>file read</th></tr>");
        for o in &f.opentype {
            let _ = write!(
                s,
                "<tr><td>{}</td><td>{}</td></tr>",
                esc(&o.name),
                esc(o.file.as_deref().unwrap_or("(not in the .fls)"))
            );
        }
        s.push_str("</table>");
    }
    if !f.preloaded.is_empty() {
        let _ = write!(s, "<p>Preloaded by the format, embedded: {}.</p>", esc(&f.preloaded.join(", ")));
    }
    if !f.unknown_embedded.is_empty() {
        let _ =
            write!(s, "<p>In the PDF, from included graphics or unknown: {}.</p>", esc(&f.unknown_embedded.join(", ")));
    }
    s.push_str("<h3>In the PDF</h3><table><tr><th>name</th><th>type</th><th>embedded</th></tr>");
    for p in &f.embedded_in_pdf {
        let _ = write!(
            s,
            "<tr><td>{}</td><td>{}</td><td>{}{}</td></tr>",
            esc(&p.name),
            esc(&p.kind),
            if p.embedded { "yes" } else { "NO" },
            if p.subset { ", subset" } else { "" }
        );
    }
    s.push_str("</table>");
    s
}

/// The whole page.
pub fn page(m: &BuildModel, job: &str, files_read: Option<usize>, fonts: Option<&FontReport>) -> String {
    let mut s = String::with_capacity(64 * 1024);
    let total = ms(m.total());
    let preamble = ms(m.preamble());
    let _ = write!(
        s,
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>prumo: {job_esc}</title>
<style>
:root {{ --bg:#fbfaf7; --fg:#1f2328; --mute:#6b7280; --line:#e2dfd8; --cls:#5b7db1; --pkg:#5fa36b; --file:#d0a540; --job:#9aa0a6; --typeset:#c9d8ee; --ship:#5b7db1; --phase:#b23a48; --warn:#8a4b00; --warnbg:#fff4e0; --accent:#2b5aa6; }}
@media (prefers-color-scheme: dark) {{ :root:not([data-theme="light"]) {{ --bg:#151719; --fg:#e6e3dc; --mute:#9aa0a6; --line:#2d3136; --cls:#7fa0d4; --pkg:#7cc488; --file:#e0bb5a; --job:#6b7075; --typeset:#2f4668; --ship:#7fa0d4; --phase:#e06b78; --warn:#f0c070; --warnbg:#2e2410; --accent:#8ab4f8; }} }}
:root[data-theme="dark"] {{ --bg:#151719; --fg:#e6e3dc; --mute:#9aa0a6; --line:#2d3136; --cls:#7fa0d4; --pkg:#7cc488; --file:#e0bb5a; --job:#6b7075; --typeset:#2f4668; --ship:#7fa0d4; --phase:#e06b78; --warn:#f0c070; --warnbg:#2e2410; --accent:#8ab4f8; }}
html,body {{ margin:0; background:var(--bg); color:var(--fg); font:14px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }}
main {{ max-width:1200px; margin:0 auto; padding:16px; }}
h1 {{ font-size:20px; margin:0 0 4px; }} h2 {{ font-size:16px; margin:24px 0 8px; }} h3 {{ font-size:14px; margin:16px 0 6px; }}
.sub {{ color:var(--mute); margin:0 0 12px; }}
nav.tabs {{ display:flex; gap:4px; border-bottom:1px solid var(--line); margin:12px 0; }}
nav.tabs button {{ background:none; border:0; border-bottom:2px solid transparent; color:var(--mute); font:inherit; padding:6px 10px; cursor:pointer; }}
nav.tabs button[aria-selected="true"] {{ color:var(--fg); border-bottom-color:var(--accent); }}
section.tab {{ display:block; }} body.js section.tab:not(.active) {{ display:none; }}
svg.flame {{ display:block; background:var(--bg); }}
svg.flame rect {{ stroke:var(--bg); stroke-width:0.6; }} svg.flame .cls rect {{ fill:var(--cls); }} svg.flame .pkg rect {{ fill:var(--pkg); }} svg.flame .file rect {{ fill:var(--file); }} svg.flame .job rect {{ fill:var(--job); }}
svg.flame g.f:hover rect {{ opacity:0.75; }}
svg.flame text {{ font-size:11px; fill:#111; pointer-events:none; font-family:ui-monospace, SFMono-Regular, Menlo, monospace; }}
svg.flame text.axis {{ fill:var(--mute); font-size:10px; }}
svg.flame line.tick {{ stroke:var(--line); stroke-width:0.6; }} svg.flame line.phase {{ stroke:var(--phase); stroke-width:1; stroke-dasharray:4 3; }}
svg.flame rect.typeset {{ fill:var(--typeset); }} svg.flame rect.ship {{ fill:var(--ship); }}
svg.flame g.fold rect {{ cursor:pointer; stroke:var(--accent); stroke-dasharray:3 2; }} svg.flame g.fold:focus {{ outline:none; }} svg.flame g.fold:focus rect {{ stroke-dasharray:none; stroke-width:1.2; }}
details.preamble {{ margin:14px 0 0; }} details.preamble summary {{ cursor:pointer; color:var(--accent); margin:0 0 6px; }}
.legend {{ color:var(--mute); font-size:12px; margin:6px 0 0; }} .legend i {{ display:inline-block; width:10px; height:10px; margin:0 4px 0 10px; vertical-align:-1px; }}
table {{ border-collapse:collapse; font-size:13px; width:100%; }} th {{ text-align:left; color:var(--mute); font-weight:600; }} td,th {{ padding:3px 8px 3px 0; border-bottom:1px solid var(--line); vertical-align:top; }}
td.n {{ text-align:right; font-variant-numeric:tabular-nums; white-space:nowrap; }} th.n {{ text-align:right; }}
.k {{ font-size:11px; padding:0 4px; border-radius:3px; color:#111; }} .k.cls {{ background:var(--cls); }} .k.pkg {{ background:var(--pkg); }} .k.file {{ background:var(--file); }} .k.job {{ background:var(--job); }}
ul.tree, ul.tree ul {{ list-style:none; padding-left:18px; margin:0; }} ul.tree li {{ margin:2px 0; }} ul.tree summary {{ cursor:pointer; }} .t {{ color:var(--mute); font-size:12px; }}
.warn {{ background:var(--warnbg); color:var(--warn); padding:6px 10px; border-radius:4px; }}
td.unused {{ color:var(--warn); }}
code {{ font-family:ui-monospace, SFMono-Regular, Menlo, monospace; font-size:12px; }}
</style></head><body><main>
<h1>prumo: {job_esc}</h1>
<p class="sub">{engine} {format}, total {total:.1} ms, preamble {preamble:.1} ms, {pages} page{pages_s}{files}</p>
<nav class="tabs" role="tablist"><button role="tab" aria-selected="true" data-tab="time">Time</button><button role="tab" aria-selected="false" data-tab="tree">Tree</button><button role="tab" aria-selected="false" data-tab="fonts">Fonts</button></nav>
<section class="tab active" id="time"><h2>Load tree over time</h2>
{flame}
<p class="legend">x: time since engine start; the preamble is one row (click it to expand); files read in the body at their depth; hover for numbers. <i style="background:var(--cls)"></i>class <i style="background:var(--pkg)"></i>package <i style="background:var(--file)"></i>file <i style="background:var(--typeset)"></i>page typesetting <i style="background:var(--ship)"></i>shipout hooks</p>
<details class="preamble" id="preamble"><summary>Preamble on its own axis: {units} classes and packages, {preamble:.1} ms to \begin{{document}}</summary>
{preamble_flame}
<p class="legend">x: time since engine start, to \begin{{document}}; rows: nesting depth; the dashed line is begindocument/before.</p>
</details>
<h2>Slowest units (exclusive)</h2><table><tr><th>kind</th><th>unit</th><th class="n">exclusive ms</th><th class="n">inclusive ms</th><th class="n">loads</th></tr>
"#,
        job_esc = esc(job),
        engine = esc(&m.engine_name),
        format = esc(&m.format_date),
        pages = m.pages.len(),
        pages_s = if m.pages.len() == 1 { "" } else { "s" },
        files = files_read.map(|n| format!(", {n} files read")).unwrap_or_default(),
        flame = flame_svg(m, View::Folded),
        units = m.preamble_units(),
        preamble_flame = flame_svg(m, View::Preamble),
    );
    for r in m.unit_table().into_iter().take(25) {
        let _ = write!(
            s,
            r#"<tr><td><span class="k {}">{}</span></td><td>{}</td><td class="n">{:.1}</td><td class="n">{:.1}</td><td class="n">{}</td></tr>"#,
            kind_class(r.kind),
            kind_word(r.kind),
            esc(&r.name),
            ms(r.exclusive),
            ms(r.inclusive),
            r.loads
        );
    }
    s.push_str("</table>");
    if !m.pages.is_empty() {
        s.push_str(r#"<h2>Pages</h2><table><tr><th class="n">page</th><th class="n">total ms</th><th class="n">typesetting</th><th class="n">shipout</th></tr>"#);
        for (n, typeset, ship) in m.page_split() {
            let _ = write!(
                s,
                r#"<tr><td class="n">{n}</td><td class="n">{:.1}</td><td class="n">{:.1}</td><td class="n">{}</td></tr>"#,
                ms(typeset + ship.unwrap_or(0)),
                ms(typeset),
                ship.map(|x| format!("{:.1}", ms(x))).unwrap_or_else(|| "–".into())
            );
        }
        s.push_str("</table>");
    }
    for w in &m.warnings {
        let _ = write!(s, r#"<p class="warn">{}</p>"#, esc(w));
    }
    s.push_str(r#"</section><section class="tab" id="tree"><h2>Which package loaded which</h2><ul class="tree">"#);
    for c in &m.root.children {
        tree_html(c, 0, &mut s);
    }
    s.push_str(r#"</ul></section><section class="tab" id="fonts"><h2>Fonts</h2>"#);
    match fonts {
        Some(f) => s.push_str(&fonts_html(f)),
        None => s.push_str("<p>No font data for this run (no PDF or no pdffonts).</p>"),
    }
    s.push_str(
        r#"</section>
<p class="sub">Made by prumo from one instrumented build; the probe only listens to LaTeX hooks and never changes typesetting.</p>
</main>
<script>
document.body.classList.add('js');
document.querySelectorAll('nav.tabs button').forEach(function (b) {
  b.addEventListener('click', function () {
    document.querySelectorAll('nav.tabs button').forEach(function (x) { x.setAttribute('aria-selected', 'false'); });
    document.querySelectorAll('section.tab').forEach(function (x) { x.classList.remove('active'); });
    b.setAttribute('aria-selected', 'true');
    document.getElementById(b.dataset.tab).classList.add('active');
  });
});
var fold = document.querySelector('svg.flame g.fold'), pre = document.getElementById('preamble');
if (fold && pre) {
  var expand = function () { pre.open = true; pre.scrollIntoView({ behavior: 'smooth', block: 'start' }); };
  fold.addEventListener('click', expand);
  fold.addEventListener('keydown', function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); expand(); } });
  pre.addEventListener('toggle', function () { fold.setAttribute('aria-expanded', pre.open ? 'true' : 'false'); });
}
</script></body></html>
"#,
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::parse_str;

    #[test]
    fn page_has_every_frame_and_escapes_names() {
        let recs = parse_str(
            "start 0 0 pdftex 2026-11-01\nfile+ 10 0 main.tex\ncls+ 20 1 article\nfile+ 21 1 article.cls\nfile+ 30 2 a&b<c>.clo\nfile- 40 2 a&b<c>.clo\nfile- 50 1 article.cls\ncls- 51 1 article\npkg+ 60 1 xcolor\nopts 60 1 table\nfile+ 61 1 xcolor.sty\nfile- 90 1 xcolor.sty\npkg- 91 1 xcolor\npredoc 100 1 \nbegindoc 110 1 \nship 150 1 1\npage 160 1 1\nenddoc 170 1 \nend 180 0 \n",
            "t",
        )
        .unwrap();
        let m = BuildModel::from_records(&recs, "t").unwrap();
        let html = page(&m, "main", Some(42), None);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>prumo: main</title>"));
        // The folded chart: the main file, the preamble row, nothing from the preamble
        // itself; the preamble chart under <details>: every frame (all of them started
        // before \begin{document} here).
        let mut frames = 0;
        m.root.walk(&mut |_, _| frames += 1);
        let (folded, expanded) = html.split_once("<details").unwrap();
        assert_eq!(folded.matches("<g class=\"f ").count(), 2, "{folded}");
        assert!(folded.contains("preamble (2 units)") && folded.contains("aria-controls=\"preamble\""));
        assert!(!folded.contains(">xcolor<"), "preamble frames are folded away");
        assert_eq!(expanded.matches("<g class=\"f ").count(), frames);
        assert!(expanded.contains("Preamble on its own axis: 2 classes and packages"));
        assert!(expanded.contains(">xcolor<") && expanded.contains("preamble ends"));
        assert!(!expanded.contains("class=\"page\""), "no pages strip on the preamble axis");
        assert!(html.contains("a&amp;b&lt;c&gt;.clo") && !html.contains("a&b<c>"));
        assert!(html.contains("[table]"));
        assert!(html.contains("1 page, 42 files read"));
        assert!(folded.contains("class=\"page\"") && folded.contains("preamble ends"));
        assert!(html.contains("No font data"));
        assert_eq!(fit_label("siunitx", 100.0).as_deref(), Some("siunitx"));
        assert_eq!(fit_label("translations", 40.0).as_deref(), Some("tran…"));
        assert_eq!(fit_label("translations", 20.0), None);
        assert_eq!(nice_step(37.0), 50.0);
        assert_eq!(nice_step(0.12), 0.1);
        assert_eq!(esc("a\"b'c"), "a&quot;b&#39;c");
    }
}
