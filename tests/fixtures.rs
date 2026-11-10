//! Every checked-in build artifact set under `fixtures/<name>/artifacts*/<engine>/` goes
//! through the parsers. No TeX installation needed. The sets are:
//! `artifacts/` (TeX Live 2026, `texlive/texlive:latest`) and
//! `artifacts-tl2023-debian/` (Debian's TeX Live 2023, from the handoff pack).

use prumo::fls::{self, InputKind, Tree, Trees};
use prumo::records::{self, Engine, Event};
use prumo::tlpdb;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The oracle's `--json` output; only the fields these tests cross-check.
#[derive(serde::Deserialize)]
struct Golden {
    total_s: f64,
    pages: Vec<(u32, f64)>,
    fonts: GoldenFonts,
}

#[derive(serde::Deserialize)]
struct GoldenFonts {
    tfm_loaded: Vec<String>,
    bitmap_pk: Vec<String>,
}

struct ArtifactSet {
    fixture: String,
    engine_dir: String,
    dir: PathBuf,
    job: String,
}

fn artifact_sets() -> Vec<ArtifactSet> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut out = Vec::new();
    for fx in std::fs::read_dir(&root).unwrap().flatten() {
        let fixture = fx.file_name().to_string_lossy().into_owned();
        for set in std::fs::read_dir(fx.path()).unwrap().flatten() {
            if !set.file_name().to_string_lossy().starts_with("artifacts") {
                continue;
            }
            for eng in std::fs::read_dir(set.path()).unwrap().flatten() {
                let dir = eng.path();
                let Some(prumo) = std::fs::read_dir(&dir)
                    .unwrap()
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| p.extension().is_some_and(|e| e == "prumo"))
                else {
                    continue;
                };
                out.push(ArtifactSet {
                    fixture: fixture.clone(),
                    engine_dir: eng.file_name().to_string_lossy().into_owned(),
                    job: prumo.file_stem().unwrap().to_string_lossy().into_owned(),
                    dir,
                });
            }
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    assert_eq!(
        out.len(),
        31,
        "3 fixtures × 2 artifact sets × 3 engines, beamer, biblatex, environments and whatif × 3, fontspec on xelatex: {:?}",
        out.iter().map(|s| &s.dir).collect::<Vec<_>>()
    );
    out
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{}: {e}", dir.join(name).display()))
}

#[test]
fn records_parse_and_validate_for_every_set() {
    for s in artifact_sets() {
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let recs =
            records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap_or_else(|e| panic!("{label}: {e}"));
        records::validate(&recs, &label).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(recs.len() > 60, "{label}: only {} records", recs.len());
        let Event::Start { engine, format_date, .. } = &recs[0].event else { panic!("{label}: no start") };
        let expected = match s.engine_dir.as_str() {
            "pdflatex" => Engine::PdfTex,
            "xelatex" => Engine::XeTex,
            "lualatex" => Engine::LuaTex,
            other => panic!("unknown engine dir {other}"),
        };
        assert_eq!(*engine, expected, "{label}");
        assert!(format_date.len() == 10 && format_date.starts_with("20"), "{label}: format date {format_date:?}");
        assert!(matches!(recs.last().unwrap().event, Event::End), "{label}: last event is not end");
        // Documented trap: main.tex is never closed; everything else is.
        let unclosed: Vec<_> = {
            let mut stack = Vec::new();
            for r in &recs {
                match &r.event {
                    Event::Open { name, .. } => stack.push(name.clone()),
                    Event::Close { .. } => {
                        stack.pop();
                    }
                    _ => {}
                }
            }
            stack
        };
        assert_eq!(unclosed, vec![format!("{}.tex", s.job)], "{label}");
        // Every pkg+ is followed by opts on these kernels (2021-06+), and depth never goes negative.
        for w in recs.windows(2) {
            if matches!(w[0].event, Event::Open { kind: records::UnitKind::Pkg, .. }) {
                assert!(matches!(w[1].event, Event::Opts(_)), "{label}:{}: pkg+ without opts", w[0].line);
            }
        }
        assert!(recs.iter().all(|r| r.depth >= 0), "{label}: negative depth");
        // Pages agree with the oracle.
        let golden: Golden = serde_json::from_str(&read(&s.dir, "golden.json")).unwrap();
        let pages: Vec<u32> =
            recs.iter().filter_map(|r| if let Event::Page(n) = r.event { Some(n) } else { None }).collect();
        assert_eq!(pages, golden.pages.iter().map(|p| p.0).collect::<Vec<_>>(), "{label}");
        let total = prumo::scaled_to_secs(recs.last().unwrap().time - recs[0].time);
        assert!((total - golden.total_s).abs() < 1e-9, "{label}: total {total} vs oracle {}", golden.total_s);
    }
}

#[test]
fn fls_parses_and_classifies_for_every_set() {
    for s in artifact_sets() {
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(fls::pwd(&lines).is_some(), "{label}: no PWD line");
        let trees = Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt"));
        assert!(trees.texmf_dist.is_some(), "{label}: kpsewhich.txt lacks TEXMFDIST");
        let files = fls::input_files(&lines, &trees);
        let by_tree = |t: Tree| files.iter().filter(|f| f.tree == t).count();
        assert!(by_tree(Tree::TexmfDist) > 15, "{label}: {} texmf-dist files", by_tree(Tree::TexmfDist));
        assert!(by_tree(Tree::Project) >= 1, "{label}: no project files");
        let fmt = files.iter().find(|f| f.kind == InputKind::Fmt).unwrap_or_else(|| panic!("{label}: no .fmt"));
        assert_eq!(fmt.tree, Tree::TexmfVar, "{label}: format at {}", fmt.path.display());
        let main = files.iter().find(|f| f.path.ends_with(format!("{}.tex", s.job))).unwrap();
        assert_eq!(main.tree, Tree::Project, "{label}");
        assert_eq!(main.kind, InputKind::Tex);
        // The probe itself came from TEXINPUTS, outside every tree.
        let probe =
            files.iter().find(|f| f.path.ends_with("prumo.sty")).unwrap_or_else(|| panic!("{label}: no prumo.sty"));
        assert_eq!(probe.tree, Tree::System, "{label}");
        // Font metrics agree with the oracle's TFM list and bitmap list.
        let golden: Golden = serde_json::from_str(&read(&s.dir, "golden.json")).unwrap();
        let tfms: BTreeSet<String> = files
            .iter()
            .filter(|f| f.kind == InputKind::Tfm)
            .map(|f| f.path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tfms.into_iter().collect::<Vec<_>>(), golden.fonts.tfm_loaded, "{label}");
        let pks = files.iter().filter(|f| f.kind == InputKind::Pk).count();
        assert_eq!(pks, golden.fonts.bitmap_pk.len(), "{label}");
    }
}

#[test]
fn tlpdb_excerpt_resolves_the_fixtures_main_packages() {
    let db = tlpdb::parse_file(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tlpdb/excerpt.tlpdb")).unwrap();
    for s in artifact_sets() {
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap();
        let trees = Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt"));
        let files = fls::input_files(&lines, &trees);
        let pkg_of = |name: &str| {
            let f = files.iter().find(|f| f.path.ends_with(name)).unwrap_or_else(|| panic!("{label}: {name} not read"));
            db.package_for(&f.path)
        };
        match s.fixture.as_str() {
            "basic" => {
                assert_eq!(pkg_of("hyperref.sty"), Some("hyperref"), "{label}");
                assert_eq!(pkg_of("t1pcr.fd"), Some("psnfss"), "{label}");
                assert_eq!(pkg_of("pcrr8t.tfm"), Some("courier"), "{label}");
                assert_eq!(pkg_of("microtype.sty"), Some("microtype"), "{label}");
            }
            "custom-class" => {
                assert_eq!(pkg_of("siunitx.sty"), Some("siunitx"), "{label}");
                assert_eq!(pkg_of("translations.sty"), Some("translations"), "{label}");
                assert_eq!(pkg_of("geometry.sty"), Some("geometry"), "{label}");
                assert_eq!(pkg_of("obra.cls"), None, "{label}: a project file is not a package");
            }
            "corpus-b" => {
                assert_eq!(pkg_of("tikz.sty"), Some("pgf"), "{label}");
            }
            "beamer" => {
                assert_eq!(pkg_of("beamer.cls"), Some("beamer"), "{label}");
                assert_eq!(pkg_of("translator.sty"), Some("translator"), "{label}");
                assert_eq!(pkg_of("booktabs.sty"), Some("booktabs"), "{label}");
            }
            "biblatex" => {
                assert_eq!(pkg_of("biblatex.sty"), Some("biblatex"), "{label}");
                assert_eq!(pkg_of("csquotes.sty"), Some("csquotes"), "{label}");
                assert_eq!(pkg_of("logreq.sty"), Some("logreq"), "{label}");
                assert_eq!(pkg_of("paper.bbl"), None, "{label}: biber's output is a project file");
            }
            "fontspec" => {
                assert_eq!(pkg_of("fontspec.sty"), Some("fontspec"), "{label}");
                assert_eq!(pkg_of("metalogo.sty"), Some("metalogo"), "{label}");
            }
            "environments" => {
                assert_eq!(pkg_of("tikz.sty"), Some("pgf"), "{label}");
                assert_eq!(pkg_of("tabularx.sty"), Some("tools"), "{label}");
                assert_eq!(pkg_of("longtable.sty"), Some("tools"), "{label}");
                assert_eq!(pkg_of("listings.sty"), Some("listings"), "{label}");
            }
            "whatif" => {
                assert_eq!(pkg_of("siunitx.sty"), Some("siunitx"), "{label}");
                assert_eq!(pkg_of("booktabs.sty"), Some("booktabs"), "{label}");
                assert_eq!(pkg_of("tikz.sty"), Some("pgf"), "{label}");
                assert_eq!(pkg_of("hyperref.sty"), Some("hyperref"), "{label}");
            }
            other => panic!("unknown fixture {other}"),
        }
        if s.fixture != "beamer" {
            assert_eq!(pkg_of("article.cls"), Some("latex"), "{label}");
        }
    }
}

/// The oracle's unit table, pages and totals, field for field, on every artifact set.
#[test]
fn model_matches_the_oracle_on_every_set() {
    #[derive(serde::Deserialize)]
    struct G {
        total_s: f64,
        preamble_s: f64,
        units: Vec<GU>,
        pages: Vec<(u32, f64)>,
    }
    #[derive(serde::Deserialize)]
    struct GU {
        kind: String,
        name: String,
        inclusive: f64,
        exclusive: f64,
        loads: u32,
    }
    for s in artifact_sets() {
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap_or_else(|e| panic!("{label}: {e}"));
        let g: G = serde_json::from_str(&read(&s.dir, "golden.json")).unwrap();
        // The oracle's seconds come out of Python float arithmetic and can differ in the
        // last digit from the exact scaled value; rule 4 says within tolerance.
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(prumo::scaled_to_secs(m.total()), g.total_s), "{label}: total");
        assert!(close(prumo::scaled_to_secs(m.preamble()), g.preamble_s), "{label}: preamble");
        let rows = m.unit_table();
        assert_eq!(rows.len(), g.units.len(), "{label}: unit count");
        for (i, (r, gu)) in rows.iter().zip(&g.units).enumerate() {
            assert_eq!((r.kind.to_string(), r.name.as_str()), (gu.kind.clone(), gu.name.as_str()), "{label}: row {i}");
            assert!(close(prumo::scaled_to_secs(r.inclusive), gu.inclusive), "{label}: {} inclusive", r.name);
            assert!(close(prumo::scaled_to_secs(r.exclusive), gu.exclusive), "{label}: {} exclusive", r.name);
            assert_eq!(r.loads, gu.loads, "{label}: {} loads", r.name);
        }
        let pages: Vec<(u32, f64)> =
            m.page_times().into_iter().map(|p| (p.page, prumo::scaled_to_secs(p.time))).collect();
        assert_eq!(pages.len(), g.pages.len(), "{label}: page count");
        for ((n, t), (gn, gt)) in pages.iter().zip(&g.pages) {
            assert!(n == gn && close(*t, *gt), "{label}: page {n}");
        }
        // Exclusive times of every frame sum to the job total: nothing is lost or counted twice.
        let mut sum = 0;
        m.root.walk(&mut |fr, _| sum += fr.exclusive());
        let main = m.main_file().unwrap();
        assert_eq!(sum, main.inclusive(), "{label}: exclusive sum vs main inclusive");
        assert!(m.warnings.is_empty(), "{label}: {:?}", m.warnings);
        // M2 acceptance, cheap to check now: obra → siunitx → translations.
        if s.fixture == "custom-class" {
            let obra = main.find("obra").unwrap();
            let siunitx = obra.children.iter().find(|c| c.name == "siunitx").expect("obra loads siunitx");
            assert!(siunitx.children.iter().any(|c| c.name == "translations"), "{label}: siunitx loads translations");
            assert_eq!(siunitx.options.as_deref(), Some(""));
            let geometry = obra.children.iter().find(|c| c.name == "geometry").unwrap();
            assert_eq!(geometry.options.as_deref(), Some("a4paper,margin=25mm"), "{label}");
        }
    }
}

/// Exploratory cross-check: files the log says were opened vs files the records say the
/// hooks saw, on the sets that carry a full log.
#[test]
fn log_file_opens_versus_hook_records() {
    for s in artifact_sets() {
        let log_path = s.dir.join(format!("{}.log", s.job));
        if !log_path.is_file() {
            continue;
        }
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let log = prumo::texlog::parse_file(&log_path).unwrap();
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let hooked: BTreeSet<String> = recs
            .iter()
            .filter_map(|r| match &r.event {
                Event::Open { kind: records::UnitKind::File, name } => {
                    Some(name.rsplit('/').next().unwrap().to_string())
                }
                _ => None,
            })
            .collect();
        let logged: BTreeSet<String> =
            log.files.iter().map(|f| f.path.rsplit('/').next().unwrap().to_string()).collect();
        let in_log_only: Vec<_> = logged.difference(&hooked).cloned().collect();
        let in_records_only: Vec<_> = hooked.difference(&logged).cloned().collect();
        assert!(log.unbalanced.is_empty(), "{label}: unbalanced at log lines {:?}", log.unbalanced);
        assert!(log.errors.is_empty(), "{label}: {:?}", log.errors);
        assert_eq!(in_records_only, Vec::<String>::new(), "{label}: hooked but not in log");
        // Only the kernel's primitive-\input reads (.aux, .out) and the probe escape the hooks;
        // biblatex reads its .def/.bbx/.cbx/.lbx with the primitive too (\blx@inputonce).
        let blx = |n: &str| {
            s.fixture == "biblatex"
                && (n.ends_with(".bbx")
                    || n.ends_with(".cbx")
                    || n.ends_with(".lbx")
                    || n.starts_with("blx-")
                    || n == "biblatex.def"
                    || n == "biblatex.cfg")
        };
        let aux = |n: &String| {
            n == "prumo.sty" || n.ends_with(".aux") || n.ends_with(".out") || n.ends_with(".toc") || blx(n)
        };
        assert!(in_log_only.iter().all(aux), "{label}: in log but never hooked: {in_log_only:?}");
        if s.fixture == "biblatex" {
            for n in ["biblatex.def", "numeric.bbx", "numeric.cbx", "english.lbx", "blx-dm.def"] {
                assert!(in_log_only.iter().any(|x| x == n), "{label}: {n} should be a log-only file: {in_log_only:?}");
            }
        }
        assert!(
            in_log_only.iter().any(|n| n.ends_with(".aux")),
            "{label}: the .aux must show up as a log-only file: {in_log_only:?}"
        );
        assert!(log.pages >= 1, "{label}");
        // The coverage report says the same from the .fls side.
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap();
        let files = fls::input_files(&lines, &Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt")));
        let c = prumo::doctor::hook_coverage(&m, &files, Some(&log));
        let outside: Vec<String> =
            c.outside_hooks.iter().map(|o| o.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert!(
            outside.iter().all(|n| n.ends_with(".aux") || n.ends_with(".out") || blx(n)),
            "{label}: outside hooks: {outside:?}"
        );
        // siunitx asks for color at begindocument/before, after tikz loaded xcolor, which
        // claims color.sty: the kernel opens the file and does not read it. LuaTeX's
        // recorder does not list the open.
        let opened: Vec<String> =
            c.opened_not_read.iter().map(|o| o.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        let expected: &[&str] = if s.fixture == "whatif" && s.engine_dir != "lualatex" { &["color.sty"] } else { &[] };
        assert_eq!(opened, expected, "{label}: opened but not read");
        if s.fixture == "biblatex" {
            assert!(outside.iter().any(|n| n == "biblatex.def"), "{label}: doctor must list biblatex.def: {outside:?}");
            assert!(
                hooked.contains("paper.bbl"),
                "{label}: the .bbl is read through the hooks (fixture claim: biber ran, latexmk reran)"
            );
        }
        assert!(c.hooked_not_in_log.is_empty(), "{label}: {:?}", c.hooked_not_in_log);
        assert!(
            c.log_errors.is_empty() && c.log_pages == m.pages.len(),
            "{label}: pages {} vs {}",
            c.log_pages,
            m.pages.len()
        );
        let t = prumo::doctor::coverage_text(&s.job, &c, true);
        assert!(t.contains("hook coverage"), "{t}");
    }
}

/// `prumo packages` on the TeX Live 2026 sets against the excerpt (which covers their
/// whole closure): nothing unresolved, the right attribution, and a frozen list for
/// basic/pdflatex.
#[test]
fn packages_resolve_with_attribution() {
    use prumo::packages::{self, Reason};
    let db = tlpdb::parse_file(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tlpdb/excerpt.tlpdb")).unwrap();
    for s in artifact_sets() {
        if !s.dir.join("kpsewhich.txt").is_file() || s.dir.to_string_lossy().contains("debian") {
            continue;
        }
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap();
        let files = fls::input_files(&lines, &Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt")));
        let r = packages::resolve(&s.job, &m, &files, &db, &s.engine_dir);
        assert!(r.unresolved.is_empty(), "{label}: unresolved {:?}", r.unresolved);
        assert!(r.local.is_empty(), "{label}");
        let find = |n: &str| r.packages.iter().find(|p| p.name == n).unwrap_or_else(|| panic!("{label}: {n} missing"));
        let via = |n: &str| match &find(n).reason {
            Reason::Used { via, .. } => via.clone(),
            other => panic!("{label}: {n} is {other:?}, not used"),
        };
        assert!(matches!(find("texlive.infra").reason, Reason::Infra), "{label}");
        assert!(matches!(find("hyphen-base").reason, Reason::Infra), "{label}");
        let engine_pkg = match s.engine_dir.as_str() {
            "pdflatex" => "pdftex",
            "xelatex" => "xetex",
            _ => "luatex",
        };
        assert!(matches!(find(engine_pkg).reason, Reason::Engine), "{label}: {engine_pkg}");
        let provider = if s.engine_dir == "xelatex" { "xetex" } else { "latex-bin" };
        assert!(
            matches!(find(provider).reason, Reason::Used { .. } | Reason::Engine | Reason::Format { .. }),
            "{label}: {provider} {:?}",
            find(provider).reason
        );
        assert!(
            matches!(find("latex").reason, Reason::Used { .. }),
            "{label}: latex.ltx is not read, but article.cls is"
        );
        match s.fixture.as_str() {
            "basic" => {
                assert_eq!(via("hyperref"), "main.tex", "{label}");
                assert_eq!(via("courier"), "(font load)", "{label}");
                assert_eq!(via("psnfss"), "main.tex", "{label}: t1pcr.fd is loaded at the top level");
                assert!(matches!(find("url").reason, Reason::Used { .. }), "{label}: hyperref loads url.sty");
                if s.engine_dir == "pdflatex" {
                    let names: Vec<&str> = r.packages.iter().map(|p| p.name.as_str()).collect();
                    eprintln!("basic/pdflatex packages ({}): {}", names.len(), names.join(" "));
                    assert!(
                        matches!(find("cm-super").reason, Reason::Used { .. }),
                        "{label}: T1 CM outlines are embedded"
                    );
                }
            }
            "custom-class" => {
                assert_eq!(via("siunitx"), "obra.cls", "{label}");
                assert_eq!(via("translations"), "siunitx", "{label}");
                // keyval.sty belongs to the graphics package and geometry loads it first on some engines.
                assert!(["graphicx", "geometry"].contains(&via("graphics").as_str()), "{label}: {}", via("graphics"));
            }
            "corpus-b" => {
                assert_eq!(via("pgf"), "main.tex".replace("main", "notes"), "{label}");
            }
            "beamer" => {
                assert_eq!(via("beamer"), "deck.tex", "{label}");
                assert_eq!(via("booktabs"), "deck.tex", "{label}");
                for n in ["pgf", "translator", "hyperref", "xcolor", "amsmath"] {
                    let v = via(n);
                    assert!(v != "deck.tex", "{label}: {n} is loaded by the class, not the document (via {v})");
                }
            }
            "biblatex" => {
                assert_eq!(via("biblatex"), "paper.tex", "{label}");
                assert_eq!(via("csquotes"), "paper.tex", "{label}");
                assert_eq!(via("logreq"), "biblatex", "{label}");
                assert_eq!(via("etoolbox"), "biblatex", "{label}");
                assert!(
                    r.packages.iter().all(|p| p.name != "biber"),
                    "{label}: TeX never reads biber's files; prune adds it"
                );
            }
            "fontspec" => {
                assert_eq!(via("fontspec"), "glyphs.tex", "{label}");
                assert_eq!(via("metalogo"), "glyphs.tex", "{label}");
                assert!(
                    !r.packages.iter().any(|p| p.name == "lm" && matches!(p.reason, Reason::Used { .. })),
                    "{label}: XeTeX records no .otf, so no lm file is read; prune's Q7 match adds lm"
                );
            }
            "environments" => {
                for n in ["pgf", "tools", "listings"] {
                    assert_eq!(via(n), "main.tex", "{label}: {n}");
                }
            }
            "whatif" => {
                for n in ["amsmath", "siunitx", "booktabs", "pgf", "hyperref"] {
                    assert_eq!(via(n), "main.tex", "{label}: {n}");
                }
            }
            _ => unreachable!(),
        }
        let used = r.packages.iter().filter(|p| matches!(p.reason, Reason::Used { .. })).count();
        assert!(used >= 5 && r.packages.len() > used, "{label}: {used} used of {}", r.packages.len());
        assert!(r.total_size > 10_000_000, "{label}: total {}", r.total_size);
        let t = packages::text(&r);
        assert!(t.contains("-- used --") && t.contains("via "), "{t}");
    }
}

/// `fonts::report` matches the oracle's font fields on every TeX Live 2026 set (which carry
/// `fontmap.txt`); `map_entries` is excluded because the artifact holds only the relevant
/// map lines.
#[test]
fn fonts_match_the_oracle() {
    use prumo::fonts::{self, Status};
    #[derive(serde::Deserialize)]
    struct G {
        fonts: GF,
    }
    #[derive(serde::Deserialize)]
    struct GF {
        tfm_loaded: Vec<String>,
        embedded: Vec<(String, String)>,
        type3: usize,
        bitmap_pk: Vec<String>,
        loaded_not_embedded: Vec<String>,
        ps_names: std::collections::BTreeMap<String, String>,
        #[serde(rename = "virtual")]
        virtual_fonts: Vec<String>,
        embedded_resolved: Vec<String>,
        loaded_not_embedded_resolved: Vec<String>,
        no_map_entry: Vec<String>,
        preloaded: Vec<String>,
        opentype: Vec<prumo::fonts::OpenTypeFont>,
        unknown_embedded: Vec<String>,
        partial: bool,
    }
    let mut checked = 0;
    for s in artifact_sets() {
        if !s.dir.join("fontmap.txt").is_file() {
            continue;
        }
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap();
        let files = fls::input_files(&lines, &Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt")));
        let r = fonts::report(
            &s.job,
            &m,
            &prumo::fonts::FontSources {
                files: &files,
                map_text: Some(&read(&s.dir, "fontmap.txt")),
                pdffonts_text: Some(&read(&s.dir, "pdffonts.txt")),
                fd_texts: &[],
                installer: prumo::doctor::Installer::TexLive,
            },
        );
        let g: G = serde_json::from_str(&read(&s.dir, "golden.json")).unwrap();
        let g = g.fonts;
        assert_eq!(r.tfm_loaded, g.tfm_loaded, "{label}: tfm_loaded");
        let emb: Vec<(String, String)> =
            r.embedded_in_pdf.iter().map(|f| (f.name.clone(), f.type_raw.clone())).collect();
        assert_eq!(emb, g.embedded, "{label}: embedded");
        assert_eq!(r.type3, g.type3, "{label}: type3");
        assert_eq!(r.bitmap_pk, g.bitmap_pk, "{label}: bitmap_pk");
        assert_eq!(r.loaded_not_embedded_naive, g.loaded_not_embedded, "{label}: naive");
        let ps: std::collections::BTreeMap<String, String> =
            r.loaded.iter().filter_map(|f| f.ps_name.clone().map(|p| (f.tfm.clone(), p))).collect();
        assert_eq!(ps, g.ps_names, "{label}: ps_names");
        let by = |st: Status| r.loaded.iter().filter(|f| f.status == st).map(|f| f.tfm.clone()).collect::<Vec<_>>();
        assert_eq!(by(Status::Virtual), g.virtual_fonts, "{label}: virtual");
        assert_eq!(by(Status::Embedded), g.embedded_resolved, "{label}: embedded_resolved");
        let mut not_emb = by(Status::NotEmbedded);
        not_emb.extend(by(Status::NoMapEntry));
        not_emb.sort();
        assert_eq!(not_emb, g.loaded_not_embedded_resolved, "{label}: loaded_not_embedded_resolved");
        assert_eq!(by(Status::NoMapEntry), g.no_map_entry, "{label}: no_map_entry");
        assert_eq!(r.preloaded, g.preloaded, "{label}: preloaded");
        assert_eq!(r.opentype, g.opentype, "{label}: opentype");
        assert_eq!(r.unknown_embedded, g.unknown_embedded, "{label}: unknown_embedded");
        assert_eq!(r.partial, g.partial, "{label}: partial");
        if s.fixture == "fontspec" {
            // Fixture claims: no TFM for the text, four OpenType fonts the engine opened itself.
            assert!(r.tfm_loaded.is_empty(), "{label}: {:?}", r.tfm_loaded);
            let names: Vec<&str> = r.opentype.iter().map(|o| o.name.as_str()).collect();
            assert_eq!(
                names,
                ["LMRoman10-Bold", "LMRoman10-Italic", "LMRoman10-Regular", "LMSans10-Regular"],
                "{label}"
            );
            assert!(r.opentype.iter().all(|o| o.file.is_none()), "{label}: XeTeX records no .otf");
            assert!(r.preloaded.is_empty(), "{label}: OpenType fonts are not 'preloaded by the format'");
            assert!(
                r.warnings.iter().any(|w| w.contains("XeTeX opens OpenType fonts itself")),
                "{label}: {:?}",
                r.warnings
            );
            assert!(fonts::text(&r).contains("OpenType, opened by the engine"), "{label}");
        } else {
            // The Unicode engines set text in Latin Modern OpenType unless fontenc says
            // otherwise (beamer's sans, biblatex's article). LuaTeX reads the fonts through
            // kpathsea, so each one pairs with its .otf, or with the .luc luaotfload cached on
            // an earlier run; XeTeX opens them itself and pairs with nothing.
            match s.engine_dir.as_str() {
                "pdflatex" => assert!(r.opentype.is_empty(), "{label}: {:?}", r.opentype),
                "lualatex" => assert!(
                    r.opentype
                        .iter()
                        .all(|o| o.file.as_deref().is_some_and(|f| f.ends_with(".otf") || f.ends_with(".luc"))),
                    "{label}: {:?}",
                    r.opentype
                ),
                _ => assert!(r.opentype.iter().all(|o| o.file.is_none()), "{label}: {:?}", r.opentype),
            }
            if s.fixture == "beamer" && s.engine_dir != "pdflatex" {
                assert!(r.opentype.iter().any(|o| o.name == "LMSans10-Regular"), "{label}: {:?}", r.opentype);
                if s.engine_dir == "lualatex" {
                    assert!(fonts::text(&r).contains("lmsans10-regular."), "{label}");
                }
            }
        }
        // Fixture claim: the Courier TFM is loaded and never embedded, on every engine.
        if s.fixture == "basic" {
            let pcr =
                r.loaded.iter().find(|f| f.tfm == "pcrr8t").unwrap_or_else(|| panic!("{label}: pcrr8t not loaded"));
            assert!(
                matches!(pcr.status, Status::NotEmbedded | Status::NoMapEntry | Status::Virtual),
                "{label}: {:?}",
                pcr.status
            );
            assert!(
                !r.embedded_in_pdf.iter().any(|f| f.name.contains("NimbusMon") || f.name.contains("Courier")),
                "{label}: Courier embedded"
            );
            if s.engine_dir == "pdflatex" {
                assert_eq!(r.type3, 0, "{label}: cm-super present, no Type 3");
                assert!(r.warnings.is_empty(), "{label}: {:?}", r.warnings);
            }
        }
        let t = fonts::text(&r);
        assert!(t.contains("-- loaded --") && t.contains("-- in the PDF --"), "{t}");
        checked += 1;
    }
    assert_eq!(checked, 22, "every TL2026 set has a fontmap.txt");
}

/// `prune::plan` on the excerpt with the three fixtures' TL2026 runs: nothing a document
/// read is removable, schemes and collections go first, dependents precede dependencies,
/// hyphenation and --keep behave, and the keep list / profile come out right.
#[test]
fn prune_plan_on_the_excerpt() {
    use prumo::prune::keep::KeepReason;
    use prumo::prune::{self, DocRun, Hyphenation, PruneOptions};
    let db = tlpdb::parse_file(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tlpdb/excerpt.tlpdb")).unwrap();
    let mut runs = Vec::new();
    for s in artifact_sets() {
        if s.dir.to_string_lossy().contains("debian")
            || !(s.engine_dir == "pdflatex"
                || (["basic", "fontspec"].contains(&s.fixture.as_str()) && s.engine_dir == "xelatex"))
        {
            continue;
        }
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, "t").unwrap();
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap();
        let files = fls::input_files(&lines, &Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt")));
        let engine =
            if s.engine_dir == "xelatex" { prumo::run::EngineCmd::Xelatex } else { prumo::run::EngineCmd::Pdflatex };
        runs.push(DocRun {
            main: s.dir.join(format!("{}.tex", s.job)),
            engine,
            latexmk: s.fixture == "biblatex",
            tools: prune::build_tools(&lines, &s.job),
            packages: prumo::packages::resolve(&s.job, &m, &files, &db, &s.engine_dir),
            pdf_fonts: prumo::fonts::parse_pdffonts(&read(&s.dir, "pdffonts.txt")),
            languages: files
                .iter()
                .filter(|f| f.kind == prumo::fls::InputKind::Ldf)
                .filter_map(|f| f.path.file_stem().map(|x| x.to_string_lossy().into_owned()))
                .collect(),
        });
    }
    assert_eq!(runs.len(), 9, "7 pdflatex + basic/xelatex + fontspec/xelatex");
    let biblatex = runs.iter().find(|r| r.main.ends_with("paper.tex")).unwrap();
    assert_eq!(biblatex.tools, ["biber"], "a .bcf was written");
    assert!(runs.iter().filter(|r| !r.main.ends_with("paper.tex")).all(|r| r.tools.is_empty()));
    let opts = PruneOptions {
        paths: vec![],
        engines: vec![],
        latexmk: false,
        keep: vec!["enumitem".into(), "no-such-pkg".into()],
        keep_hyphenation: Hyphenation::All,
        out: PathBuf::from("/tmp/x"),
        env: vec![],
    };
    let p = prune::plan(&db, &runs, &opts);
    assert_eq!(p.documents.len(), 9);
    assert_eq!(p.installed, 128, "130 stanzas minus the two 00texlive.* ones");
    // Everything any document read stays; the acceptance list from docs/07-phases.md.
    for n in [
        "hyperref",
        "amsmath",
        "microtype",
        "xcolor",
        "psnfss",
        "courier",
        "siunitx",
        "translations",
        "pgf",
        "latex",
        "latex-bin",
        "pdftex",
        "xetex",
        "texlive.infra",
        "kpathsea",
        "hyphen-base",
        "cm-super",
        "lm",
        "beamer",
        "translator",
        "biblatex",
        "biber",
        "latexmk",
        "fontspec",
        "metalogo",
    ] {
        assert!(p.needed.contains_key(n), "{n} must be kept: {:?}", p.needed.get(n));
        assert!(
            !p.removable.contains(&n.to_string()) && !p.meta.contains(&n.to_string()),
            "{n} is in the removal list"
        );
    }
    assert!(matches!(p.needed["hyphen-portuguese"], KeepReason::Hyphenation { .. }), "all hyphenation kept by default");
    assert!(matches!(p.needed["enumitem"], KeepReason::User));
    assert!(p.warnings.iter().any(|w| w.contains("no-such-pkg")), "{:?}", p.warnings);
    assert!(
        matches!(&p.needed["siunitx"], KeepReason::Document { via, .. } if via == "obra.cls"),
        "{:?}",
        p.needed["siunitx"]
    );
    assert!(
        matches!(&p.needed["lm"], KeepReason::Document { .. } | KeepReason::OpenTypeProvider { .. }),
        "{:?}",
        p.needed["lm"]
    );
    assert!(matches!(&p.needed["biber"], KeepReason::Tool { doc } if doc.contains("paper")), "{:?}", p.needed["biber"]);
    assert!(!p.needed.contains_key("bibtex") && !p.needed.contains_key("makeindex"));
    // The fontspec document alone (XeTeX reads no lm file): lm is kept anyway, because the
    // xelatex format's fmttriggers name it; Q7 would keep it too, by matching the PDF's
    // LMRoman10-Regular-Identity-H to lmroman10-regular.otf.
    let only_fontspec: Vec<DocRun> = runs.iter().filter(|r| r.main.ends_with("glyphs.tex")).cloned().collect();
    assert_eq!(only_fontspec.len(), 1);
    let pf = prune::plan(&db, &only_fontspec, &opts);
    assert!(
        matches!(&pf.needed["lm"], KeepReason::Format { .. } | KeepReason::OpenTypeProvider { .. }),
        "{:?}",
        pf.needed.get("lm")
    );
    assert!(!pf.warnings.iter().any(|w| w.contains("could not be matched")), "{:?}", pf.warnings);
    for f in only_fontspec[0].pdf_fonts.iter().filter(|f| f.kind.contains("CID")) {
        assert_eq!(prune::keep::opentype_providers(&db, &f.name), ["lm"], "{}", f.name);
    }
    assert!(!pf.needed.contains_key("metalogo") || matches!(pf.needed["metalogo"], KeepReason::Document { .. }));
    // Q7: the XeTeX run names LMRoman10-Regular; lm ships lmroman10-regular.otf, so no fallback warning.
    assert!(!p.warnings.iter().any(|w| w.contains("could not be matched")), "{:?}", p.warnings);
    // Meta first: every scheme and collection not needed.
    assert!(
        p.meta.contains(&"scheme-full".to_string()) && p.meta.contains(&"collection-basic".to_string()),
        "{:?}",
        p.meta
    );
    assert!(p.meta[0].starts_with("scheme-"));
    // Something is removable, and dependents come before their dependencies.
    assert!(!p.removable.is_empty(), "{:?}", p.removable);
    let pos: std::collections::HashMap<&str, usize> =
        p.removable.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    for n in &p.removable {
        for d in &db.packages[n].depends {
            if let Some(&j) = pos.get(d.as_str()) {
                assert!(pos[n.as_str()] < j, "{n} depends on {d} but comes after it");
            }
        }
    }
    assert!(p.needed_size > 0 && p.removable_size > 0 && p.installed_size >= p.needed_size + p.removable_size);
    // keep.txt has base names only; the profile is scheme-infraonly.
    let keep = prune::keep_list(&p);
    assert!(keep.lines().any(|l| l == "hyperref") && !keep.contains("x86_64-linux"), "{keep}");
    assert!(prune::install_profile(&p).contains("selected_scheme scheme-infraonly"));
    let cmds = prune::commands(&p, 200);
    assert!(
        cmds[0][..3] == ["tlmgr", "remove", "--no-depends"] && cmds.iter().flatten().any(|n| n.starts_with("scheme-"))
    );
    assert_eq!(p.order.len(), p.meta.len() + p.removable.len());
    let pos: std::collections::HashMap<&str, usize> =
        p.order.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    for n in &p.order {
        for d in &db.packages[n].depends {
            if let Some(&j) = pos.get(d.as_str()) {
                assert!(pos[n.as_str()] < j, "{n} depends on {d} but comes after it in the removal order");
            }
        }
    }
    assert!(
        !p.removable
            .iter()
            .any(|n| n.ends_with(".x86_64-linux")
                && p.removable.contains(&n.trim_end_matches(".x86_64-linux").to_string())),
        "arch stanzas go with their base package"
    );
    let t = prune::text(&p, Path::new("/tmp/x"));
    assert!(t.contains("dry run") && t.contains("Removable:"), "{t}");
    // --keep-hyphenation used: no .ldf in these runs, so hyphen-portuguese goes.
    let used =
        prune::plan(&db, &runs, &PruneOptions { keep_hyphenation: Hyphenation::Used, keep: vec![], ..opts.clone() });
    assert!(!used.needed.contains_key("hyphen-portuguese"), "{:?}", used.needed.get("hyphen-portuguese"));
    assert!(used.needed.contains_key("hyphen-base"));
}

/// The TL2026 artifacts carry `ship` events: every page splits into typesetting + shipout,
/// the split sums to the page time, and JSON exposes `shipout_ms`.
#[test]
fn pages_split_into_typesetting_and_shipout() {
    for s in artifact_sets() {
        if s.dir.to_string_lossy().contains("debian") {
            continue;
        }
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        assert!(m.phases.predoc.is_some() && m.phases.predoc <= m.phases.begindoc, "{label}");
        let split = m.page_split();
        let times = m.page_times();
        assert_eq!(split.len(), times.len(), "{label}");
        for (p, t) in split.iter().zip(&times) {
            assert_eq!(p.page, t.page);
            let ship = p.ship.unwrap_or_else(|| panic!("{label}: page {} has no ship event", p.page));
            assert_eq!(p.typeset + ship, t.time, "{label}: page {}", p.page);
        }
        let j = serde_json::to_value(prumo::report::time_json(&m, &s.job)).unwrap();
        assert!(j["pages"][0]["shipout_ms"].is_number(), "{label}: {}", j["pages"][0]);
    }
}

/// `--html` on every artifact set: a page with one chart group per frame, every unit name
/// escaped, and the fonts tab filled from the same inputs as `prumo fonts`.
#[test]
fn html_page_for_every_set() {
    for s in artifact_sets() {
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        let lines = fls::parse_file(&s.dir.join(format!("{}.fls", s.job))).unwrap();
        let files = fls::input_files(&lines, &Trees::from_kpsewhich(&read(&s.dir, "kpsewhich.txt")));
        let fonts = prumo::fonts::report(
            &s.job,
            &m,
            &prumo::fonts::FontSources {
                files: &files,
                map_text: std::fs::read_to_string(s.dir.join("fontmap.txt")).ok().as_deref(),
                pdffonts_text: std::fs::read_to_string(s.dir.join("pdffonts.txt")).ok().as_deref(),
                fd_texts: &[],
                installer: prumo::doctor::Installer::TexLive,
            },
        );
        let html = prumo::html::page(&m, &s.job, Some(files.len()), Some(&fonts));
        // Every frame is drawn once: preamble frames on the preamble chart, body frames on
        // the folded one. The main file is on both, and the folded preamble row is a group
        // of its own, so two more.
        let mut frames = 0;
        m.root.walk(&mut |_, _| frames += 1);
        assert_eq!(html.matches("<g class=\"f ").count(), frames + 2, "{label}");
        assert!(html.contains("<details class=\"preamble\"") && html.contains("g.fold"), "{label}");
        assert!(html.contains(&format!("<title>prumo: {}</title>", s.job)), "{label}");
        assert!(html.contains("<h2>Fonts</h2>") && html.contains("In the PDF"), "{label}");
        assert!(
            !html.contains("<script src") && !html.contains("http://") && !html.contains("https://"),
            "{label}: self-contained"
        );
        if s.fixture == "fontspec" {
            assert!(html.contains("OpenType, opened by the engine"), "{label}");
        }
        assert!(html.len() < 2_000_000, "{label}: {} bytes", html.len());
    }
}

/// The oracle's phases, environment table and discarded shipouts, on every set whose golden
/// has them (the TeX Live 2026 sets, made with probe v0.2).
#[test]
fn phases_and_environments_match_the_oracle() {
    #[derive(serde::Deserialize)]
    struct G {
        phases: Option<GP>,
        #[serde(default)]
        environments: Vec<GE>,
        #[serde(default)]
        discarded_shipouts: Vec<u32>,
    }
    #[derive(serde::Deserialize)]
    struct GP {
        startup_s: f64,
        preamble_s: Option<f64>,
        begin_document_s: Option<f64>,
        typesetting_s: Option<f64>,
        shipout_s: f64,
        end_document_s: Option<f64>,
        after_last_page_s: Option<f64>,
        end_s: f64,
    }
    #[derive(serde::Deserialize)]
    struct GE {
        name: String,
        count: u32,
        total_s: f64,
        max_s: f64,
    }
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let secs = |x: Option<prumo::Scaled>| x.map(prumo::scaled_to_secs);
    let mut checked = 0;
    for s in artifact_sets() {
        let label = format!("{}/{}", s.fixture, s.engine_dir);
        let g: G = serde_json::from_str(&read(&s.dir, "golden.json")).unwrap();
        let Some(gp) = g.phases else {
            assert!(s.dir.to_string_lossy().contains("debian"), "{label}: golden without phases");
            continue;
        };
        checked += 1;
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        assert!(m.phases.docstart.is_some() && m.phases.lastpage.is_some(), "{label}: probe v0.2 records");
        let p = m.run_phases();
        assert!(close(prumo::scaled_to_secs(p.startup), gp.startup_s), "{label}: startup");
        for (what, ours, theirs) in [
            ("preamble", secs(p.preamble), gp.preamble_s),
            ("begin-document", secs(p.begin_document), gp.begin_document_s),
            ("typesetting", secs(p.typesetting), gp.typesetting_s),
            ("end-document", secs(p.end_document), gp.end_document_s),
            ("after last page", secs(p.after_last_page), gp.after_last_page_s),
        ] {
            match (ours, theirs) {
                (Some(a), Some(b)) => assert!(close(a, b), "{label}: {what} {a} vs oracle {b}"),
                (a, b) => panic!("{label}: {what} {a:?} vs oracle {b:?}"),
            }
        }
        assert!(close(prumo::scaled_to_secs(p.shipout), gp.shipout_s), "{label}: shipout");
        assert!(close(prumo::scaled_to_secs(p.last_record), gp.end_s), "{label}: last record");
        let sum = p.startup
            + p.preamble.unwrap()
            + p.begin_document.unwrap()
            + p.typesetting.unwrap()
            + p.shipout
            + p.end_document.unwrap();
        assert_eq!(sum, p.last_record, "{label}: the phases add up to the last record");
        let envs = m.env_table();
        assert_eq!(envs.len(), g.environments.len(), "{label}: environments");
        for (r, ge) in envs.iter().zip(&g.environments) {
            assert_eq!((r.name.as_str(), r.count), (ge.name.as_str(), ge.count), "{label}");
            assert!(close(prumo::scaled_to_secs(r.total), ge.total_s), "{label}: {} total", r.name);
            assert!(close(prumo::scaled_to_secs(r.max), ge.max_s), "{label}: {} max", r.name);
        }
        assert_eq!(m.discarded_ships, g.discarded_shipouts, "{label}: discarded shipouts");
        let j = serde_json::to_value(prumo::report::time_json(&m, &s.job)).unwrap();
        assert_eq!(j["timer"]["clock"], "wall", "{label}");
        assert!(j["phases"]["finalization_ms"].is_null(), "{label}: no wall time without a run");
        assert!(j["phases"]["startup_ms"].as_f64().unwrap() > 0.0, "{label}");
    }
    assert_eq!(checked, 22, "every TeX Live 2026 set");
}

/// fixtures/environments/main.tex: every claim in its header, on all three engines.
#[test]
fn environments_fixture_claims() {
    let sets: Vec<ArtifactSet> = artifact_sets().into_iter().filter(|s| s.fixture == "environments").collect();
    assert_eq!(sets.len(), 3);
    for s in sets {
        let label = s.engine_dir.clone();
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        // 1. tikzpicture 4 times (1 + 3 tabularx passes), the others once.
        let envs = m.env_table();
        let count = |n: &str| envs.iter().find(|r| r.name == n).map(|r| r.count);
        assert_eq!(count("tikzpicture"), Some(4), "{label}: {envs:?}");
        for n in ["tabularx", "longtable", "lstlisting"] {
            assert_eq!(count(n), Some(1), "{label}: {n}");
        }
        // 2. Only those four names; the inline \tikz forms leave nothing.
        assert_eq!(envs.len(), 4, "{label}: {envs:?}");
        let in_tabularx = m.envs.iter().filter(|e| e.name == "tikzpicture").count();
        assert_eq!(in_tabularx, 4);
        let tabularx = m.envs.iter().find(|e| e.name == "tabularx").unwrap();
        let inside =
            m.envs.iter().filter(|e| e.name == "tikzpicture" && e.start >= tabularx.start && e.end <= tabularx.end);
        assert_eq!(inside.count(), 3, "{label}: three passes inside tabularx");
        // 3. 3 pages, 4 shipouts started, the second discarded.
        assert_eq!(m.pages.len(), 3, "{label}");
        let ships = recs.iter().filter(|r| matches!(r.event, Event::Ship(_))).count();
        assert_eq!(ships, 4, "{label}");
        assert_eq!(m.discarded_ships, [2], "{label}");
        // 4. Page 2 ships inside the longtable, and the longtable's time leaves it out.
        let lt = m.envs.iter().find(|e| e.name == "longtable").unwrap();
        let p2 = m.pages.iter().find(|p| p.number == 2).unwrap();
        assert!(lt.start < p2.ship_start.unwrap() && p2.time < lt.end, "{label}: page 2 inside longtable");
        let lt_row = envs.iter().find(|r| r.name == "longtable").unwrap();
        assert_eq!(lt_row.total, (lt.end - lt.start) - (p2.time - p2.ship_start.unwrap()), "{label}");
        // 5. \AtBeginDocument between predoc and docstart; page 1 counts from docstart.
        let (predoc, docstart) = (m.phases.predoc.unwrap(), m.phases.docstart.unwrap());
        assert!(predoc < docstart && m.phases.begindoc.unwrap() <= docstart, "{label}");
        assert_eq!(m.page_times()[0].time, m.pages[0].time - docstart, "{label}");
        // 6. Page 3 after enddoc, before lastpage.
        let p3 = m.pages.iter().find(|p| p.number == 3).unwrap();
        assert!(m.phases.enddoc.unwrap() < p3.ship_start.unwrap(), "{label}");
        assert!(p3.time <= m.phases.lastpage.unwrap(), "{label}");
        // 7 is tests/tex.rs::probe_leaves_the_pdf_byte_identical.
    }
}

/// fixtures/whatif/main.tex, claim 7: the bounds `whatif` prints come from the probe's
/// records. The rest of its header is tests/tex.rs::whatif_fixture_claims.
#[test]
fn whatif_fixture_bounds() {
    use prumo::whatif::{Experiment, bound_ms};
    let sets: Vec<ArtifactSet> = artifact_sets().into_iter().filter(|s| s.fixture == "whatif").collect();
    assert_eq!(sets.len(), 3);
    for s in sets {
        let label = s.engine_dir.clone();
        let recs = records::parse_file(&s.dir.join(format!("{}.prumo", s.job))).unwrap();
        let m = prumo::model::BuildModel::from_records(&recs, &label).unwrap();
        let envs = m.env_table();
        let tikz = envs.iter().find(|r| r.name == "tikzpicture").unwrap();
        assert_eq!(tikz.count, 1, "{label}: one picture");
        assert_eq!(bound_ms(&m, Experiment::Externalize), Some(prumo::report::ms(tikz.total)), "{label}");
        let p = m.run_phases();
        let preamble = p.preamble.unwrap();
        assert_eq!(bound_ms(&m, Experiment::Preamble), Some(prumo::report::ms(preamble)), "{label}");
        // Startup is left out: on LuaLaTeX the engine's start is about as long as this
        // preamble, and CI regenerates these artifacts on its own runner before testing.
        let later = [p.begin_document, p.typesetting, Some(p.shipout), p.end_document];
        assert!(
            later.iter().all(|o| o.unwrap() < preamble),
            "{label}: the preamble is longer than every phase after it: {p:?}"
        );
        assert_eq!(bound_ms(&m, Experiment::Draft), None);
    }
}

/// `scripts/make_artifacts.sh` loads the probe the way prumo does.
#[test]
fn make_artifacts_watches_the_default_environments() {
    let script =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/make_artifacts.sh")).unwrap();
    let line = script.lines().find(|l| l.starts_with("envs=")).expect("envs= line");
    assert_eq!(line.trim_start_matches("envs="), prumo::run::DEFAULT_ENVIRONMENTS.join(","));
}

/// `prumo time output.prumo`: a record file on its own (Overleaf hides the `.fls`) is read
/// with no build and nothing written next to it.
#[test]
fn records_alone_are_read_without_a_build() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/environments/artifacts/lualatex/main.prumo");
    let dir = tempfile::tempdir().unwrap();
    let records = dir.path().join("output.prumo");
    std::fs::copy(&src, &records).unwrap();
    let out = prumo::run::from_records(&records).unwrap();
    assert_eq!((out.job.as_str(), out.info.engine), ("output", prumo::run::EngineCmd::Lualatex));
    assert!(out.pdf.is_none() && !out.fls.exists());
    let m = prumo::run::model_of(&out).unwrap();
    assert_eq!(m.pages.len(), 3);
    assert!(m.run_phases().wall.is_none());
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names, ["output.prumo"], "nothing written next to the records");
    assert!(prumo::run::from_records(&dir.path().join("nope.prumo")).is_err());
}
