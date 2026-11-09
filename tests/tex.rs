//! Tests that need a real TeX installation: `cargo test --features tex`.
//! On a machine without one, use `scripts/tex_tests.sh`, which builds this binary on the
//! host and runs it inside the image from `docker/Dockerfile.tex`.
#![cfg(feature = "tex")]

use prumo::model::BuildModel;
use prumo::run::{self, EngineCmd, RunOptions};
use std::path::{Path, PathBuf};
use std::sync::Once;

const ENGINES: [EngineCmd; 3] = [EngineCmd::Pdflatex, EngineCmd::Xelatex, EngineCmd::Lualatex];
/// Fixture, job, engines it builds on (fontspec pins XeTeX with a magic comment; biblatex
/// has a single-pass set here, its latexmk claims are in `biblatex_through_latexmk`).
const FIXTURES: [(&str, &str, &[EngineCmd]); 6] = [
    ("basic", "main", &ENGINES),
    ("custom-class", "main", &ENGINES),
    ("corpus-b", "notes", &ENGINES),
    ("beamer", "deck", &ENGINES),
    ("biblatex", "paper", &ENGINES),
    ("fontspec", "glyphs", &[EngineCmd::Xelatex]),
];

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Deterministic output and a warm luaotfload cache (a cold one records every system font).
fn setup() -> Vec<(String, String)> {
    static WARM: Once = Once::new();
    WARM.call_once(|| {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("w.tex"), "\\documentclass{article}\\begin{document}x\\end{document}").unwrap();
        let _ = run::build_once(dir.path(), "w", &run::BuildCmd::standard(EngineCmd::Lualatex, false), None, &[], None);
    });
    vec![("SOURCE_DATE_EPOCH".into(), "0".into()), ("FORCE_SOURCE_DATE".into(), "1".into())]
}

/// A fresh copy of a fixture's sources in a temp dir.
fn fixture_copy(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(fixtures_root().join(name)).unwrap().flatten() {
        if e.path().is_file() {
            std::fs::copy(e.path(), dir.path().join(e.file_name())).unwrap();
        }
    }
    dir
}

/// `run()` on every fixture × engine: artifacts land in the cache, the user's copy is
/// untouched, and the tree has the same units as the checked-in artifacts.
#[test]
fn run_every_fixture_on_every_engine() {
    let env = setup();
    for (fx, job, engines) in FIXTURES {
        for &engine in engines {
            let label = format!("{fx}/{}", engine.command());
            let dir = fixture_copy(fx);
            let before: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
            let main = dir.path().join(format!("{job}.tex"));
            // The biblatex artifacts were made through latexmk (biber, then the .bbl is read).
            let latexmk = fx == "biblatex";
            let ro = RunOptions {
                main: main.clone(),
                engine,
                latexmk,
                build: None,
                out: None,
                env: env.clone(),
                pretex: None,
                edits: vec![],
                timeout: None,
            };
            let out = run::run(&ro).unwrap_or_else(|e| panic!("{label}: {e}"));
            assert_eq!(out.info.exit_code, Some(0), "{label}: see {}", out.dir.join("build.out").display());
            let after: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
            assert_eq!(before, after, "{label}: the project directory changed");
            assert!(
                out.dir.starts_with(run::cache_root()),
                "{label}: artifacts outside the cache: {}",
                out.dir.display()
            );
            assert!(
                out.dir.join("run.json").is_file()
                    && out.dir.join("kpsewhich.txt").is_file()
                    && out.dir.join("pdffonts.txt").is_file(),
                "{label}"
            );
            let recs = prumo::records::parse_file(&out.records).unwrap();
            prumo::records::validate(&recs, &label).unwrap();
            let m = BuildModel::from_records(&recs, &label).unwrap();
            let fresh: std::collections::BTreeSet<String> =
                m.unit_table().into_iter().map(|r| format!("{} {}", r.kind, r.name)).collect();
            let checked_in = prumo::records::parse_file(
                &fixtures_root().join(fx).join("artifacts").join(engine.command()).join(format!("{job}.prumo")),
            )
            .unwrap();
            let expected: std::collections::BTreeSet<String> = BuildModel::from_records(&checked_in, "checked-in")
                .unwrap()
                .unit_table()
                .into_iter()
                .map(|r| format!("{} {}", r.kind, r.name))
                .collect();
            assert_eq!(fresh, expected, "{label}: unit set differs from the checked-in artifacts");
            // M1 acceptance: exclusive times sum to the total within 1%.
            let mut sum = 0;
            m.root.walk(&mut |fr, _| sum += fr.exclusive());
            let total = m.main_file().unwrap().inclusive();
            assert!(sum.abs_diff(total) * 100 <= total, "{label}: exclusive sum {sum} vs total {total}");
            // The cache is fresh right after the build and stale after a source edit.
            let cached = run::cached(&main, None).unwrap().expect("cache");
            assert!(!run::is_stale(&cached, &ro), "{label}: fresh cache reported stale");
            let other = if engine == EngineCmd::Pdflatex { EngineCmd::Xelatex } else { EngineCmd::Pdflatex };
            assert!(
                run::is_stale(&cached, &RunOptions { engine: other, ..ro.clone() }),
                "{label}: engine change not stale"
            );
            assert!(
                run::is_stale(&cached, &RunOptions { latexmk: !latexmk, ..ro.clone() }),
                "{label}: latexmk not stale"
            );
            assert!(
                run::is_stale(&cached, &RunOptions { build: Some("pdflatex -shell-escape".into()), ..ro.clone() }),
                "{label}: --build not stale"
            );
            // The build's own outputs (.aux, .out) are not sources: touching them changes nothing.
            std::thread::sleep(std::time::Duration::from_millis(1100));
            std::fs::write(dir.path().join(format!("{job}.aux")), "\\relax\n").unwrap();
            assert!(!run::is_stale(&cached, &ro), "{label}: a fresh .aux made the cache stale");
            std::thread::sleep(std::time::Duration::from_millis(1100));
            let text = std::fs::read_to_string(&main).unwrap();
            std::fs::write(&main, text).unwrap();
            assert!(run::is_stale(&cached, &ro), "{label}: edited source not stale");
        }
    }
}

/// Two documents with the same job name sharing one `--out` must not see each other's run.
#[test]
fn cache_is_keyed_by_document_not_just_job_name() {
    let env = setup();
    let out = tempfile::tempdir().unwrap();
    let a = fixture_copy("basic");
    let b = fixture_copy("custom-class");
    let a_main = a.path().join("main.tex");
    let b_main = b.path().join("main.tex");
    run::run(&RunOptions {
        main: a_main.clone(),
        engine: EngineCmd::Pdflatex,
        latexmk: false,
        build: None,
        out: Some(out.path().to_path_buf()),
        env,
        pretex: None,
        edits: vec![],
        timeout: None,
    })
    .unwrap();
    assert!(run::cached(&a_main, Some(out.path())).unwrap().is_some());
    assert!(run::cached(&b_main, Some(out.path())).unwrap().is_none(), "custom-class must not get basic's run");
}

/// Rule 3 of the charter: the probe never changes typesetting. Same directory for both
/// builds (LuaTeX's trailer /ID hashes the output path), `SOURCE_DATE_EPOCH=0`.
#[test]
fn probe_leaves_the_pdf_byte_identical() {
    let env = setup();
    for (fx, job, engines) in FIXTURES {
        for &engine in engines {
            let label = format!("{fx}/{}", engine.command());
            let dir = fixture_copy(fx);
            let keep: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
            let clean = |d: &Path| {
                for e in std::fs::read_dir(d).unwrap().flatten() {
                    if !keep.contains(&e.file_name()) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            };
            let (_, code) =
                run::build_once(dir.path(), job, &run::BuildCmd::standard(engine, false), None, &env, None).unwrap();
            assert_eq!(code, Some(0), "{label}: plain build failed");
            let plain = std::fs::read(dir.path().join(format!("{job}.pdf"))).unwrap();
            clean(dir.path());
            let probe = tempfile::tempdir().unwrap();
            std::fs::write(probe.path().join("prumo.sty"), run::PROBE).unwrap();
            let (_, code) = run::build_once(
                dir.path(),
                job,
                &run::BuildCmd::standard(engine, false),
                Some(probe.path()),
                &env,
                None,
            )
            .unwrap();
            assert_eq!(code, Some(0), "{label}: probed build failed");
            let probed = std::fs::read(dir.path().join(format!("{job}.pdf"))).unwrap();
            assert!(dir.path().join(format!("{job}.prumo")).is_file(), "{label}: no record file");
            assert!(plain == probed, "{label}: PDF differs with the probe ({} vs {} bytes)", plain.len(), probed.len());
        }
    }
}

/// The latexmk route writes the same record file (last pass wins).
#[test]
fn latexmk_route_records_too() {
    let env = setup();
    let dir = fixture_copy("basic");
    let main = dir.path().join("main.tex");
    let out = run::run(&RunOptions {
        main,
        engine: EngineCmd::Pdflatex,
        latexmk: true,
        build: None,
        out: None,
        env,
        pretex: None,
        edits: vec![],
        timeout: None,
    })
    .unwrap();
    assert_eq!(out.info.exit_code, Some(0), "see {}", out.dir.join("build.out").display());
    assert!(out.info.command[0] == "latexmk");
    let recs = prumo::records::parse_file(&out.records).unwrap();
    let m = BuildModel::from_records(&recs, "latexmk").unwrap();
    assert_eq!(m.pages.len(), 2);
    assert!(m.main_file().unwrap().find("hyperref").is_some());
}

/// M3 acceptance: the basic fixture reports `pcrr8t` loaded-not-embedded on every engine;
/// on pdfTeX it reports Type 3 fonts (with the fix hint) when cm-super is absent and a clean
/// report when it is present. Which case runs depends on the image (`docker/Dockerfile.tex`
/// targets `clean` and `nocmsuper`); the test checks the installation and asserts the
/// matching branch, so both images exercise it.
#[test]
fn fonts_report_courier_and_type3() {
    use prumo::fonts::{self, Status};
    let env = setup();
    let cm_super = std::process::Command::new("kpsewhich")
        .arg("sfrm1000.pfb")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    for engine in ENGINES {
        let label = engine.command();
        let dir = fixture_copy("basic");
        let main = dir.path().join("main.tex");
        let out = run::run(&RunOptions {
            main,
            engine,
            latexmk: false,
            build: None,
            out: None,
            env: env.clone(),
            pretex: None,
            edits: vec![],
            timeout: None,
        })
        .unwrap();
        assert_eq!(out.info.exit_code, Some(0), "{label}");
        assert!(out.dir.join("fontmap.txt").is_file(), "{label}: run saved no fontmap.txt");
        assert!(out.dir.join("fd/t1pcr.fd").is_file(), "{label}: run saved no .fd files");
        let recs = prumo::records::parse_file(&out.records).unwrap();
        let m = BuildModel::from_records(&recs, label).unwrap();
        let lines = prumo::fls::parse_file(&out.fls).unwrap();
        let files = prumo::fls::input_files(
            &lines,
            &prumo::fls::Trees::from_kpsewhich(&std::fs::read_to_string(out.dir.join("kpsewhich.txt")).unwrap()),
        );
        let fd: Vec<(String, String)> = std::fs::read_dir(out.dir.join("fd"))
            .unwrap()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
            .collect();
        let r = fonts::report(
            &out.job,
            &m,
            &prumo::fonts::FontSources {
                files: &files,
                map_text: std::fs::read_to_string(out.dir.join("fontmap.txt")).ok().as_deref(),
                pdffonts_text: std::fs::read_to_string(out.dir.join("pdffonts.txt")).ok().as_deref(),
                fd_texts: &fd,
                installer: prumo::doctor::Installer::TexLive,
            },
        );
        let pcr = r.loaded.iter().find(|f| f.tfm == "pcrr8t").unwrap_or_else(|| panic!("{label}: pcrr8t not loaded"));
        assert_ne!(pcr.status, Status::Embedded, "{label}");
        assert_eq!(
            pcr.declared_by.as_deref(),
            Some("T1/pcr via t1pcr.fd, loaded by main.tex"),
            "{label}: attribution through the .fd frame"
        );
        if engine == EngineCmd::Pdflatex {
            if cm_super {
                assert_eq!(r.type3, 0, "{label}: cm-super installed, no Type 3");
                assert!(r.bitmap_pk.is_empty(), "{label}");
                assert!(r.warnings.is_empty(), "{label}: {:?}", r.warnings);
                assert!(
                    r.loaded.iter().any(|f| f.tfm == "ecrm1000"
                        && f.status == Status::Embedded
                        && f.ps_name.as_deref() == Some("SFRM1000")),
                    "{label}: {:?}",
                    r.loaded
                );
            } else {
                assert!(r.type3 > 0, "{label}: no cm-super, expected Type 3 fonts: {:?}", r.embedded_in_pdf);
                assert!(
                    !r.bitmap_pk.is_empty(),
                    "{label}: .pk files must have been generated: {:?}",
                    files.iter().filter(|f| f.kind == prumo::fls::InputKind::Pk).collect::<Vec<_>>()
                );
                assert!(r.partial);
                let w = r.warnings.join("\n");
                assert!(w.contains("Type 3") && w.contains("cm-super"), "{label}: {w}");
                assert!(fonts::text(&r).contains("WARNING"));
            }
        }
    }
}

/// `prumo compare`: three rows, all ok, warm run reported, first run recorded.
#[test]
fn compare_runs_every_engine() {
    use prumo::compare::{self, CompareOptions, Outcome};
    let env = setup();
    let dir = fixture_copy("basic");
    let main = dir.path().join("main.tex");
    let mut seen = Vec::new();
    let c = compare::compare(
        &CompareOptions {
            main: main.clone(),
            engines: compare::ALL_ENGINES.to_vec(),
            latexmk: false,
            runs: 2,
            out: None,
            env,
        },
        |m| seen.push(m.to_string()),
    )
    .unwrap();
    assert!(seen[0].starts_with("artifacts in "), "{seen:?}");
    assert_eq!(seen.len(), 7, "one location line, then two runs per engine: {seen:?}");
    assert_eq!(c.rows.len(), 3);
    for r in &c.rows {
        match &r.outcome {
            Outcome::Ok { total_ms, pages, first_run_ms, slowest_unit, .. } => {
                assert!(*total_ms > 0.0 && *pages == 2, "{:?}", r);
                assert!(first_run_ms.is_some(), "{:?}", r);
                assert!(slowest_unit.is_some(), "{:?}", r);
            }
            other => panic!("{}: {other:?}", r.engine.command()),
        }
    }
    let per_engine = run::default_out(&dir.path().join("main.tex")).unwrap().join("compare/xelatex/main/run.json");
    assert!(per_engine.is_file(), "per-engine cache: {}", per_engine.display());
    assert!(!dir.path().join(".prumo").exists(), "nothing next to the document");
    let t = compare::text(&c);
    assert!(t.contains("hint:") && t.contains("pdflatex") && t.contains("lualatex"), "{t}");
    let j = serde_json::to_string(&c).unwrap();
    assert!(j.contains("\"status\":\"ok\""), "{j}");
}

/// M5 acceptance (docs/07-phases.md): basic → xcolor no observable effect, amsmath required,
/// microtype not tested; custom-class → graphicx and enumitem (via obra.cls) no observable
/// effect, siunitx (via obra.cls) required. The basic fixture runs in under 10 s.
#[test]
fn unused_verdicts_on_the_fixtures() {
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let t0 = std::time::Instant::now();
    let dir = fixture_copy("basic");
    let opts = UnusedOptions {
        main: dir.path().join("main.tex"),
        engine: EngineCmd::Pdflatex,
        latexmk: false,
        build: None,
        jobs: 2,
        compare: Compare::Text,
        test_all: false,
        timeout_secs: 0,
        out: None,
        env: env.clone(),
    };
    let r = unused::run(&opts, |_| {}).unwrap();
    let verdict = |p: &str| {
        r.candidates
            .iter()
            .find(|c| c.package == p)
            .unwrap_or_else(|| panic!("{p} not a candidate: {:?}", r.candidates))
            .verdict
            .clone()
    };
    assert_eq!(verdict("xcolor"), Verdict::NoObservableEffect);
    assert!(matches!(verdict("amsmath"), Verdict::Required { .. }), "{:?}", verdict("amsmath"));
    assert!(matches!(verdict("microtype"), Verdict::NotTested { .. }), "{:?}", verdict("microtype"));
    assert!(matches!(verdict("fontenc"), Verdict::NotTested { .. }));
    assert!(matches!(verdict("hyperref"), Verdict::NotTested { .. }));
    assert!(t0.elapsed().as_secs() < 10, "took {:?}", t0.elapsed());
    assert!(!r.cached);
    // Second call: same sources → from the cache, no builds.
    let r2 = unused::run(&opts, |m| assert!(m.starts_with("artifacts in "), "should not build: {m}")).unwrap();
    assert!(r2.cached && r2.candidates == r.candidates);
    // --test-all: hyperref is actually required here (\href), fontenc only affects output.
    let all = unused::run(&UnusedOptions { test_all: true, ..opts.clone() }, |_| {}).unwrap();
    let v = |p: &str| all.candidates.iter().find(|c| c.package == p).unwrap().verdict.clone();
    assert!(matches!(v("hyperref"), Verdict::Required { .. }), "{:?}", v("hyperref"));
    assert!(!matches!(v("microtype"), Verdict::NotTested { .. }));
    let t = unused::text(&all);
    assert!(t.contains("no observable effect") && t.contains("xcolor"), "{t}");

    let dir = fixture_copy("custom-class");
    let r =
        unused::run(&UnusedOptions { main: dir.path().join("main.tex"), out: None, ..opts.clone() }, |_| {}).unwrap();
    let c = |p: &str| r.candidates.iter().find(|c| c.package == p).unwrap_or_else(|| panic!("{p}: {:?}", r.candidates));
    assert_eq!(c("graphicx").verdict, Verdict::NoObservableEffect);
    assert_eq!(c("graphicx").loaded_by, "main.tex");
    assert_eq!(c("enumitem").verdict, Verdict::NoObservableEffect);
    assert_eq!(c("enumitem").loaded_by, "obra.cls");
    assert!(matches!(c("siunitx").verdict, Verdict::Required { .. }), "{:?}", c("siunitx").verdict);
    assert_eq!(c("siunitx").loaded_by, "obra.cls");
    assert!(matches!(c("geometry").verdict, Verdict::NotTested { .. }));
    assert_eq!(c("geometry").options, "a4paper,margin=25mm");
    assert!(unused::text(&r).contains("obra.cls loads"), "{}", unused::text(&r));
    assert!(
        !r.candidates.iter().any(|c| c.package == "color" || c.package == "epstopdf-base"),
        "hook loads are not candidates: {:?}",
        r.candidates
    );
    // Render mode agrees on the clear cases.
    let rr =
        unused::run(&UnusedOptions { main: dir.path().join("main.tex"), compare: Compare::Render, ..opts }, |_| {})
            .unwrap();
    assert_eq!(rr.candidates.iter().find(|c| c.package == "enumitem").unwrap().verdict, Verdict::NoObservableEffect);
}

/// A package that only draws must not be called unused: corpus-b is one TikZ picture and no
/// text, so `pdftotext` sees nothing either way; the ink check has to catch it.
#[test]
fn unused_ink_check_catches_a_drawing_package() {
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let dir = fixture_copy("corpus-b");
    let r = unused::run(
        &UnusedOptions {
            main: dir.path().join("notes.tex"),
            engine: EngineCmd::Pdflatex,
            latexmk: false,
            build: None,
            jobs: 2,
            compare: Compare::Text,
            test_all: false,
            timeout_secs: 0,
            out: None,
            env,
        },
        |_| {},
    )
    .unwrap();
    let tikz = r.candidates.iter().find(|c| c.package == "tikz").expect("tikz is a candidate");
    match &tikz.verdict {
        Verdict::AffectsOutput { evidence } => assert!(evidence.contains("ink differs"), "{evidence}"),
        Verdict::Required { evidence } => {
            assert!(evidence.contains("tikzpicture") || evidence.contains("Undefined"), "{evidence}")
        }
        other => panic!("tikz: {other:?}"),
    }
}

/// `--build`: the user's own command. Engine `-output-directory` with `-shell-escape`,
/// latexmk `-outdir` + `-auxdir` (records in aux, PDF in out), a bare `latexmk` whose
/// engine only the start record reveals, and a `-jobname` that renames the artifacts.
#[test]
fn build_command_with_output_directories() {
    let env = setup();
    let cases = [
        ("pdflatex -shell-escape -output-directory=build", EngineCmd::Pdflatex, "main"),
        ("latexmk -xelatex -outdir=out -auxdir=aux main.tex", EngineCmd::Xelatex, "main"),
        ("latexmk", EngineCmd::Pdflatex, "main"),
        ("lualatex -jobname=thesis", EngineCmd::Lualatex, "thesis"),
    ];
    for (cmd, engine, job) in cases {
        let dir = fixture_copy("basic");
        std::fs::create_dir_all(dir.path().join("build")).unwrap();
        let ro = RunOptions {
            main: dir.path().join("main.tex"),
            engine: EngineCmd::Xelatex, // ignored: the command decides
            latexmk: false,
            build: Some(cmd.into()),
            out: None,
            env: env.clone(),
            pretex: None,
            edits: vec![],
            timeout: None,
        };
        let out = run::run(&ro).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        assert_eq!(out.info.exit_code, Some(0), "{cmd}: see {}", out.dir.join("build.out").display());
        assert_eq!(out.info.engine, engine, "{cmd}");
        assert_eq!(out.info.build.as_deref(), Some(cmd));
        assert_eq!(out.job, job, "{cmd}");
        // A bare latexmk follows latexmkrc; the test image's default is DVI, so no PDF there.
        if cmd != "latexmk" {
            assert!(out.pdf.is_some(), "{cmd}: no PDF collected");
            assert!(out.dir.join("pdffonts.txt").is_file(), "{cmd}");
        }
        let argv = out.info.command.join(" ");
        assert!(argv.contains("-recorder") && argv.contains("RequirePackage{prumo}"), "{cmd}: {argv}");
        if cmd.contains("shell-escape") {
            assert!(argv.contains("-shell-escape"), "{argv}");
        }
        assert!(!argv.contains(" main.tex -") && !argv.ends_with("main.tex main.tex"), "{cmd}: {argv}");
        let recs = prumo::records::parse_file(&out.records).unwrap();
        let m = BuildModel::from_records(&recs, cmd).unwrap();
        assert_eq!(m.pages.len(), 2, "{cmd}");
        let cached = run::cached(&ro.main, None).unwrap().expect("cache");
        assert!(!run::is_stale(&cached, &ro), "{cmd}: fresh cache stale");
        assert!(
            run::is_stale(&cached, &RunOptions { build: Some(format!("{cmd} -halt-on-error")), ..ro.clone() }),
            "{cmd}: a different command must rebuild"
        );
        assert!(
            run::is_stale(&cached, &RunOptions { build: None, ..ro.clone() }),
            "{cmd}: dropping --build must rebuild"
        );
        if cmd.starts_with("latexmk -xelatex") {
            assert!(!dir.path().join("out").exists() && !dir.path().join("aux").exists(), "project untouched");
        }
    }
    let e = run::run(&RunOptions {
        main: fixture_copy("basic").path().join("main.tex"),
        engine: EngineCmd::Pdflatex,
        latexmk: false,
        build: Some("tectonic".into()),
        out: None,
        env,
        pretex: None,
        edits: vec![],
        timeout: None,
    })
    .unwrap_err();
    assert!(matches!(e, run::RunError::BadBuild { .. }), "{e}");
}

/// beamer fixture claims: 5 shipouts for a title frame and three frames with one \pause;
/// beamer's own loads sit under the class; `\usepackage{amsmath}` is redundant (already
/// loaded, no rebuild); booktabs has no observable effect; tikz is required.
#[test]
fn beamer_deck_claims() {
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let dir = fixture_copy("beamer");
    let main = dir.path().join("deck.tex");
    let r = unused::run(
        &UnusedOptions {
            main: main.clone(),
            engine: EngineCmd::Pdflatex,
            latexmk: false,
            build: None,
            jobs: 2,
            compare: Compare::Text,
            test_all: false,
            timeout_secs: 0,
            out: None,
            env,
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(r.baseline_pages, 5, "a frame is not a page: title + One (2, pause) + Two + Three");
    let c = |p: &str| r.candidates.iter().find(|c| c.package == p).unwrap_or_else(|| panic!("{p}: {:?}", r.candidates));
    assert_eq!(c("booktabs").verdict, Verdict::NoObservableEffect);
    assert!(matches!(c("tikz").verdict, Verdict::Required { .. }), "{:?}", c("tikz").verdict);
    assert!(
        matches!(&c("amsmath").verdict, Verdict::AlreadyLoaded { by } if by.starts_with("beamer.cls")),
        "{:?}",
        c("amsmath").verdict
    );
    assert_eq!(c("amsmath").loaded_by, "deck.tex");
    assert!(c("amsmath").build_secs.is_none(), "nothing to rebuild for a redundant line");
    assert!(
        !r.candidates.iter().any(|c| ["hyperref", "xcolor", "pgfcore"].contains(&c.package.as_str())),
        "the class's loads are not the document's candidates: {:?}",
        r.candidates
    );
    let t = unused::text(&r);
    assert!(t.contains("redundant") && t.contains("already loaded by beamer.cls"), "{t}");
    let cached = run::cached(&main, None).unwrap().unwrap();
    let recs = prumo::records::parse_file(&cached.records).unwrap();
    let m = BuildModel::from_records(&recs, "beamer").unwrap();
    let main_file = m.main_file().unwrap();
    let beamer = main_file.children.iter().find(|c| c.name == "beamer").expect("beamer.cls under deck.tex");
    assert!(beamer.find("hyperref").is_some() && beamer.find("xcolor").is_some(), "beamer loads hyperref and xcolor");
    assert!(main_file.children.iter().all(|c| c.name != "hyperref"));
}

/// biblatex fixture claims, through latexmk: biber runs, the last pass reads paper.bbl
/// through the hooks, the PDF cites [1] and lists Knuth; csquotes has no observable effect;
/// the single-pass build still records; biblatex's .def/.bbx files escape the hooks and
/// `doctor --check-log` says so.
#[test]
fn biblatex_through_latexmk() {
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let dir = fixture_copy("biblatex");
    let main = dir.path().join("paper.tex");
    let ro = RunOptions {
        main: main.clone(),
        engine: EngineCmd::Pdflatex,
        latexmk: true,
        build: None,
        out: None,
        env: env.clone(),
        pretex: None,
        edits: vec![],
        timeout: None,
    };
    let out = run::run(&ro).unwrap();
    assert_eq!(out.info.exit_code, Some(0), "see {}", out.dir.join("build.out").display());
    let build_out = std::fs::read_to_string(out.dir.join("build.out")).unwrap();
    assert!(build_out.contains("biber") || build_out.contains("Biber"), "latexmk must have run biber: {build_out}");
    let recs = prumo::records::parse_file(&out.records).unwrap();
    let m = BuildModel::from_records(&recs, "biblatex").unwrap();
    assert!(m.main_file().unwrap().find("paper.bbl").is_some(), "the .bbl is read through the hooks");
    let text = std::process::Command::new("pdftotext")
        .args([out.pdf.as_ref().unwrap().to_str().unwrap(), "-"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("[1]") && text.contains("Knuth"), "citation resolved: {text}");
    // doctor --check-log: biblatex.def and friends are read with the primitive \input.
    let lines = prumo::fls::parse_file(&out.fls).unwrap();
    let trees = prumo::fls::Trees::from_kpsewhich(&std::fs::read_to_string(out.dir.join("kpsewhich.txt")).unwrap());
    let files = prumo::fls::input_files(&lines, &trees);
    let log = prumo::texlog::parse_file(&out.log).ok();
    let c = prumo::doctor::hook_coverage(&m, &files, log.as_ref());
    let outside: Vec<String> =
        c.outside_hooks.iter().map(|o| o.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(outside.iter().any(|n| n == "biblatex.def") && outside.iter().any(|n| n == "numeric.bbx"), "{outside:?}");
    assert_eq!(prumo::prune::build_tools(&lines, "paper"), ["biber"]);
    // unused through latexmk: csquotes is not used by a book title.
    let r = unused::run(
        &UnusedOptions {
            main: main.clone(),
            engine: EngineCmd::Pdflatex,
            latexmk: true,
            build: None,
            jobs: 2,
            compare: Compare::Text,
            test_all: false,
            timeout_secs: 0,
            out: None,
            env: env.clone(),
        },
        |_| {},
    )
    .unwrap();
    let c = |p: &str| r.candidates.iter().find(|c| c.package == p).unwrap_or_else(|| panic!("{p}: {:?}", r.candidates));
    assert_eq!(c("csquotes").verdict, Verdict::NoObservableEffect);
    assert!(matches!(c("biblatex").verdict, Verdict::Required { .. }), "{:?}", c("biblatex").verdict);
    assert!(!r.candidates.iter().any(|c| c.package == "url" || c.package == "etoolbox"), "{:?}", r.candidates);
    // One engine pass, no biber: still builds and records, no bibliography.
    let dir2 = fixture_copy("biblatex");
    let out2 = run::run(&RunOptions { main: dir2.path().join("paper.tex"), latexmk: false, env, ..ro }).unwrap();
    assert_eq!(out2.info.exit_code, Some(0));
    let recs2 = prumo::records::parse_file(&out2.records).unwrap();
    let m2 = BuildModel::from_records(&recs2, "single").unwrap();
    assert!(m2.main_file().unwrap().find("paper.bbl").is_none());
    assert_eq!(m2.pages.len(), 1);
}

/// fontspec fixture claims on XeTeX: no TFM for the text, four OpenType fonts the engine
/// opened itself (not in the .fls, so unpaired, with the warning); url has no observable
/// effect; metalogo is required.
#[test]
fn fontspec_on_xetex() {
    use prumo::fonts;
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let dir = fixture_copy("fontspec");
    let main = dir.path().join("glyphs.tex");
    let out = run::run(&RunOptions {
        main: main.clone(),
        engine: EngineCmd::Xelatex,
        latexmk: false,
        build: None,
        out: None,
        env: env.clone(),
        pretex: None,
        edits: vec![],
        timeout: None,
    })
    .unwrap();
    assert_eq!(out.info.exit_code, Some(0), "see {}", out.dir.join("build.out").display());
    let recs = prumo::records::parse_file(&out.records).unwrap();
    let m = BuildModel::from_records(&recs, "fontspec").unwrap();
    let lines = prumo::fls::parse_file(&out.fls).unwrap();
    let files = prumo::fls::input_files(
        &lines,
        &prumo::fls::Trees::from_kpsewhich(&std::fs::read_to_string(out.dir.join("kpsewhich.txt")).unwrap()),
    );
    assert!(
        !files.iter().any(|f| matches!(f.kind, prumo::fls::InputKind::Otf | prumo::fls::InputKind::Ttf)),
        "XeTeX records no font files"
    );
    let r = fonts::report(
        &out.job,
        &m,
        &prumo::fonts::FontSources {
            files: &files,
            map_text: std::fs::read_to_string(out.dir.join("fontmap.txt")).ok().as_deref(),
            pdffonts_text: std::fs::read_to_string(out.dir.join("pdffonts.txt")).ok().as_deref(),
            fd_texts: &[],
            installer: prumo::doctor::Installer::TexLive,
        },
    );
    assert!(r.tfm_loaded.is_empty(), "{:?}", r.tfm_loaded);
    let names: Vec<&str> = r.opentype.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["LMRoman10-Bold", "LMRoman10-Italic", "LMRoman10-Regular", "LMSans10-Regular"]);
    assert!(r.opentype.iter().all(|o| o.file.is_none()));
    assert!(r.preloaded.is_empty() && r.unknown_embedded.is_empty(), "{:?} {:?}", r.preloaded, r.unknown_embedded);
    assert!(r.warnings.iter().any(|w| w.contains("XeTeX opens OpenType fonts itself")), "{:?}", r.warnings);
    assert!(r.embedded_in_pdf.iter().all(|f| f.kind.contains("CID") && f.embedded), "{:?}", r.embedded_in_pdf);
    let u = unused::run(
        &UnusedOptions {
            main,
            engine: EngineCmd::Xelatex,
            latexmk: false,
            build: None,
            jobs: 2,
            compare: Compare::Text,
            test_all: false,
            timeout_secs: 0,
            out: None,
            env,
        },
        |_| {},
    )
    .unwrap();
    let c = |p: &str| u.candidates.iter().find(|c| c.package == p).unwrap_or_else(|| panic!("{p}: {:?}", u.candidates));
    assert_eq!(c("url").verdict, Verdict::NoObservableEffect);
    assert!(matches!(c("metalogo").verdict, Verdict::Required { .. }), "{:?}", c("metalogo").verdict);
    assert!(matches!(c("fontspec").verdict, Verdict::NotTested { .. }));
}

/// `unused --corpus`: obra.cls judged against its documents. With main.tex and a trivial
/// note, enumitem is removable; add a document that uses enumitem and it is needed there.
#[test]
fn unused_corpus_judges_a_class_across_documents() {
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let dir = fixture_copy("custom-class");
    std::fs::write(
        dir.path().join("nota.tex"),
        "\\documentclass{obra}\n\\begin{document}\n\\obratitle{Nota}\nUma nota curta.\n\\end{document}\n",
    )
    .unwrap();
    let opts = UnusedOptions {
        main: PathBuf::new(),
        engine: EngineCmd::Pdflatex,
        latexmk: false,
        build: None,
        jobs: 2,
        compare: Compare::Text,
        test_all: false,
        timeout_secs: 0,
        out: None,
        env: env.clone(),
    };
    let loader = dir.path().join("obra.cls");
    let docs =
        vec![dir.path().join("main.tex").display().to_string(), dir.path().join("nota.tex").display().to_string()];
    let r = unused::run_corpus(&loader, &docs, &opts, |_| {}).unwrap();
    assert_eq!(r.loader, "obra.cls");
    assert_eq!(r.documents.len(), 2);
    assert!(r.documents.iter().all(|d| d.uses_loader), "{:?}", r.documents);
    let p = |n: &str| r.packages.iter().find(|p| p.package == n).unwrap_or_else(|| panic!("{n}: {:?}", r.packages));
    assert!(p("enumitem").removable, "{:?}", p("enumitem"));
    assert_eq!(p("enumitem").verdicts.len(), 2);
    assert!(!p("siunitx").removable && p("siunitx").needed_by.iter().any(|d| d.ends_with("main.tex")));
    assert!(matches!(
        p("siunitx").verdicts.values().find(|_| true),
        Some(Verdict::Required { .. } | Verdict::NoObservableEffect)
    ));
    assert!(!p("geometry").removable && p("geometry").untested_in.len() == 2);
    let t = unused::corpus_text(&r);
    assert!(t.contains("obra.cls can drop 1 package: enumitem"), "{t}");
    // A third document that uses enumitem: not removable any more, and it names the document.
    std::fs::write(
        dir.path().join("lista.tex"),
        "\\documentclass{obra}\n\\begin{document}\n\\begin{enumerate}[label=(\\alph*)]\\item um\\item dois\\end{enumerate}\n\\end{document}\n",
    )
    .unwrap();
    let docs = vec![dir.path().display().to_string()];
    let r = unused::run_corpus(&loader, &docs, &opts, |_| {}).unwrap();
    assert_eq!(r.documents.len(), 3, "{:?}", r.documents);
    let e = r.packages.iter().find(|p| p.package == "enumitem").unwrap();
    assert!(!e.removable && e.needed_by.len() == 1 && e.needed_by[0].ends_with("lista.tex"), "{e:?}");
    assert!(r.documents.iter().filter(|d| d.cached).count() >= 2, "the first two documents come from their caches");
    assert!(unused::corpus_text(&r).contains("every package obra.cls loads is needed"), "{}", unused::corpus_text(&r));
    // Not a file: a clear error.
    assert!(matches!(
        unused::run_corpus(&dir.path().join("nope.cls"), &docs, &opts, |_| {}),
        Err(unused::UnusedError::NoLoader(_))
    ));
}

/// A timeout kills the build instead of labeling it afterwards (a candidate that loops on
/// A package the document uses and another package loads anyway (graphicx under tikz, as in
/// florilegium's manual): the request is redundant, the package is not unused, and the
/// report says which load supplies it.
#[test]
fn loaded_anyway_is_told_apart_from_unused() {
    use prumo::unused::{self, Compare, UnusedOptions, Verdict};
    let env = setup();
    let dir = fixture_copy("corpus-b");
    std::fs::write(
        dir.path().join("anyway.tex"),
        "\\documentclass{article}\n\\usepackage{graphicx}\n\\usepackage{tikz}\n\\begin{document}\n\\scalebox{1.5}{wide} \\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}\n\\end{document}\n",
    )
    .unwrap();
    let r = unused::run(
        &UnusedOptions {
            main: dir.path().join("anyway.tex"),
            engine: EngineCmd::Pdflatex,
            latexmk: false,
            build: None,
            jobs: 2,
            compare: Compare::Text,
            test_all: false,
            timeout_secs: 0,
            out: None,
            env,
        },
        |_| {},
    )
    .unwrap();
    let c = |p: &str| r.candidates.iter().find(|c| c.package == p).unwrap_or_else(|| panic!("{p}: {:?}", r.candidates));
    assert!(
        matches!(&c("graphicx").verdict, Verdict::LoadedAnyway { by } if by.starts_with("tikz.sty")),
        "{:?}",
        c("graphicx").verdict
    );
    assert!(
        !matches!(c("tikz").verdict, Verdict::NoObservableEffect | Verdict::LoadedAnyway { .. }),
        "{:?}",
        c("tikz").verdict
    );
    let t = unused::text(&r);
    assert!(t.contains("loaded anyway by tikz.sty") && t.contains("loads it without this line"), "{t}");
}

/// Without `--engine`, the CLI takes the document's `% !TEX program =` line (the fontspec
/// fixture says xelatex), says so, and `--engine` still wins.
#[test]
fn magic_comment_picks_the_engine_on_the_cli() {
    let _ = setup();
    let dir = fixture_copy("fontspec");
    let out = dir.path().join("cache");
    let prumo = env!("CARGO_BIN_EXE_prumo");
    let o = std::process::Command::new(prumo)
        .args(["run", "glyphs.tex", "--out"])
        .arg(&out)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let c = run::cached(&dir.path().join("glyphs.tex"), Some(&out)).unwrap().expect("a cached run");
    assert_eq!(c.info.engine, EngineCmd::Xelatex, "{:?}", c.info.command);
    let o = std::process::Command::new(prumo)
        .args(["time", "glyphs.tex", "--engine", "pdflatex", "--top", "1", "--out"])
        .arg(&out)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("building glyphs.tex with pdflatex"), "--engine wins: {err}");
}

/// errors in nonstopmode ran 124 s on florilegium before this existed).
#[test]
fn timeout_kills_the_build() {
    let env = setup();
    let dir = fixture_copy("basic");
    let t0 = std::time::Instant::now();
    let e = run::run(&RunOptions {
        main: dir.path().join("main.tex"),
        engine: EngineCmd::Pdflatex,
        latexmk: true,
        build: None,
        out: None,
        env,
        pretex: None,
        edits: vec![],
        timeout: Some(std::time::Duration::from_millis(1)),
    })
    .unwrap_err();
    assert!(matches!(e, run::RunError::TimedOut { .. }), "{e}");
    assert!(t0.elapsed().as_secs() < 5, "killed promptly, latexmk's engine child included: {:?}", t0.elapsed());
}
