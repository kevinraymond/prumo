//! `prumo graph`: the load tree as text, DOT or JSON.

use crate::model::{BuildModel, Frame, FrameKind};
use crate::report::ms;
use std::fmt::Write as _;

/// Indented tree, every level, options shown for packages that had any.
pub fn tree_text(m: &BuildModel, depth: usize) -> String {
    let mut s = String::new();
    let mut hidden = 0usize;
    m.root.walk(&mut |fr, d| {
        if d >= depth {
            hidden += 1;
            return;
        }
        let opts = match &fr.options {
            Some(o) if !o.is_empty() => format!(" [{o}]"),
            _ => String::new(),
        };
        let _ = writeln!(
            s,
            "{}{} {}{opts}  ({:.1} ms incl, {:.1} ms excl)",
            "  ".repeat(d),
            fr.kind,
            fr.name,
            ms(fr.inclusive()),
            ms(fr.exclusive())
        );
    });
    if hidden > 0 {
        let _ = writeln!(s, "... {hidden} deeper frames hidden (--depth)");
    }
    s
}

/// Graphviz DOT. Packages and classes are boxes, files are plain; edge = "loaded".
/// Node labels carry inclusive and exclusive time; node colors darken with exclusive share.
pub fn dot(m: &BuildModel, job: &str, depth: usize) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "digraph prumo {{");
    let _ =
        writeln!(s, "  label=\"{} — {} ms total\"; labelloc=t; rankdir=LR;", esc(job), crate::scaled_to_ms(m.total()));
    let _ = writeln!(s, "  node [fontname=\"Helvetica\", fontsize=10];");
    let total = m.total().max(1) as f64;
    let mut n = 0usize;
    fn go(fr: &Frame, parent: Option<usize>, d: usize, depth: usize, n: &mut usize, total: f64, s: &mut String) {
        if d >= depth {
            return;
        }
        for c in &fr.children {
            *n += 1;
            let id = *n;
            let share = c.exclusive() as f64 / total;
            let shape = match c.kind {
                FrameKind::Pkg => "box",
                FrameKind::Cls => "box, peripheries=2",
                _ => "plaintext",
            };
            let fill = if share > 0.005 {
                format!(", style=filled, fillcolor=\"0.08 {:.2} 1.0\"", (share * 3.0).min(1.0))
            } else {
                String::new()
            };
            let _ = writeln!(
                s,
                "  n{id} [label=\"{}\\n{:.1} ms incl / {:.1} ms excl\", shape={shape}{fill}];",
                esc(&c.name),
                ms(c.inclusive()),
                ms(c.exclusive())
            );
            if let Some(p) = parent {
                let _ = writeln!(s, "  n{p} -> n{id};");
            }
            go(c, Some(id), d + 1, depth, n, total, s);
        }
    }
    go(&m.root, None, 0, depth, &mut n, total, &mut s);
    s.push_str("}\n");
    s
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `prumo graph --json`: the tree with options, schema 1.
#[derive(Debug, serde::Serialize)]
pub struct GraphJson<'a> {
    pub schema: u32,
    pub job: &'a str,
    pub engine: String,
    pub total_ms: f64,
    pub tree: Vec<crate::report::TreeJson>,
    /// Every parent → child pair by name, in load order; the flat form for tools.
    pub edges: Vec<Edge>,
}

#[derive(Debug, serde::Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: FrameKind,
}

pub fn graph_json<'a>(m: &'a BuildModel, job: &'a str) -> GraphJson<'a> {
    let t = crate::report::time_json(m, job);
    let mut edges = Vec::new();
    fn go(fr: &Frame, from: &str, edges: &mut Vec<Edge>) {
        for c in &fr.children {
            edges.push(Edge { from: from.to_string(), to: c.name.clone(), kind: c.kind });
            go(c, &c.name, edges);
        }
    }
    for c in &m.root.children {
        go(c, &c.name, &mut edges);
    }
    GraphJson { schema: 1, job, engine: m.engine.to_string(), total_ms: t.total_ms, tree: t.tree, edges }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::parse_str;

    fn model() -> BuildModel {
        BuildModel::from_records(
            &parse_str(
                "start 0 0 pdftex\nfile+ 0 0 main.tex\ncls+ 10 1 obra\nfile+ 11 1 obra.cls\npkg+ 20 2 siunitx\nopts 20 2 a=b\nfile+ 21 2 siunitx.sty\npkg+ 30 3 translations\nopts 30 3 \nfile+ 31 3 translations.sty\nfile- 40 3 translations.sty\npkg- 41 3 translations\nfile- 50 2 siunitx.sty\npkg- 51 2 siunitx\nfile- 60 1 obra.cls\ncls- 61 1 obra\nend 100 0 \n",
                "t",
            )
            .unwrap(),
            "t",
        )
        .unwrap()
    }

    #[test]
    fn tree_dot_and_edges() {
        let m = model();
        let t = tree_text(&m, 10);
        assert!(t.contains("    pkg siunitx [a=b]"), "{t}");
        assert!(t.contains("      pkg translations  ("), "empty options are not shown: {t}");
        let t2 = tree_text(&m, 2);
        assert!(t2.contains("deeper frames hidden"));
        let d = dot(&m, "main", 10);
        assert!(d.starts_with("digraph prumo {"));
        assert!(d.contains("n2 -> n3;") && d.contains("n3 -> n4;"), "{d}");
        assert!(d.contains("peripheries=2"), "class node: {d}");
        let g = graph_json(&m, "main");
        let e: Vec<String> = g.edges.iter().map(|e| format!("{}>{}", e.from, e.to)).collect();
        assert_eq!(e, ["main.tex>obra", "obra>siunitx", "siunitx>translations"]);
    }
}
