use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use prumo::model::BuildModel;
use prumo::report::{self, TimeBy};
use prumo::run::{self, EngineCmd, RunOptions, RunOutput};
use std::path::PathBuf;

/// prumo — build analyzer for LaTeX: where the time goes, which package loaded which, which fonts
/// and TeX Live packages a document needs.
#[derive(Parser)]
#[command(name = "prumo", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Flags shared by every command that needs a build.
#[derive(clap::Args, Clone)]
struct BuildArgs {
    /// The document (`main.tex`). Its directory is copied; nothing in it is modified.
    main: PathBuf,
    /// Engine to run. Default: the document's `% !TEX program =` magic comment, else pdflatex.
    #[arg(long, value_enum)]
    engine: Option<EngineCmd>,
    /// Drive the build with latexmk (reruns, biber, makeindex) instead of one engine pass.
    #[arg(long)]
    latexmk: bool,
    /// Your own build command, without the document: `--build "latexmk -pdf -shell-escape
    /// -outdir=build"`. prumo adds -recorder, -jobname and the probe, and reads the
    /// artifacts from -output-directory / -outdir / -auxdir. Replaces --engine and --latexmk.
    #[arg(long, value_name = "CMD", conflicts_with_all = ["engine", "latexmk"])]
    build: Option<String>,
    /// Artifact directory (default: a per-document directory under the cache root, see
    /// `prumo where`; `PRUMO_CACHE` moves the root). Nothing is written next to the document.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Rebuild even if the cached run looks fresh.
    #[arg(long)]
    rebuild: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum GraphFormat {
    Tree,
    Dot,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum By {
    Unit,
    Package,
    File,
    Page,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check the TeX installation: engines, kernel date, -recorder, Poppler tools, tlpdb.
    /// Exit code 0 when `prumo run` can work here, 1 otherwise.
    Doctor {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// Instead of the installation, check a document's cached run: files read outside
        /// the LaTeX file hooks, and the log against the records.
        #[arg(long, value_name = "MAIN.TEX")]
        check_log: Option<PathBuf>,
        /// Artifact directory of that run (default: the cache directory, see `prumo where`).
        #[arg(long, requires = "check_log")]
        out: Option<PathBuf>,
    },
    /// What the cache holds, and delete it: everything, one document's entry, or `--stale`
    /// (documents that no longer exist). Lists only, unless `--yes`.
    Clean {
        /// Only this document's entry.
        main: Option<PathBuf>,
        /// Only entries whose document is gone.
        #[arg(long, conflicts_with = "main")]
        stale: bool,
        /// Delete what is listed.
        #[arg(long)]
        yes: bool,
        /// Machine-readable listing (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// Where prumo keeps a document's artifacts (or the cache root, with no document).
    Where {
        /// The document; without it, the cache root.
        main: Option<PathBuf>,
        /// Artifact directory, if the run used `--out`.
        #[arg(long, requires = "main")]
        out: Option<PathBuf>,
    },
    /// Run the instrumented build and cache its artifacts under `<out>/<job>/`.
    Run {
        #[command(flatten)]
        build: BuildArgs,
    },
    /// Per-package, per-file and per-page time (runs the build if the cache is missing or stale).
    Time {
        #[command(flatten)]
        build: BuildArgs,
        /// Group by LaTeX unit (packages, classes, bare files), packages only, file, or page.
        #[arg(long, value_enum, default_value_t = By::Unit)]
        by: By,
        /// Rows in the table.
        #[arg(long, default_value_t = 12)]
        top: usize,
        /// Levels of the load tree to print.
        #[arg(long, default_value_t = 3)]
        depth: usize,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
        /// Plain text, no bars or timeline (the default when stdout is not a UTF-8 terminal;
        /// NO_COLOR keeps the charts and drops the color).
        #[arg(long, conflicts_with = "json")]
        plain: bool,
        /// Expand the preamble in the timeline: its own axis, to `\begin{document}`, one
        /// row per package, class and file (folded into one row by default).
        #[arg(long, conflicts_with = "json")]
        preamble: bool,
        /// Write a self-contained HTML page (flame chart, tree, fonts) to FILE, or to
        /// `<job>-prumo.html` in the current directory when no FILE is given.
        #[arg(long, value_name = "FILE", num_args = 0..=1, conflicts_with = "json")]
        html: Option<Option<PathBuf>>,
    },
    /// Which package loaded which: the load tree as text, Graphviz DOT, or JSON.
    Graph {
        #[command(flatten)]
        build: BuildArgs,
        /// Output format.
        #[arg(long, value_enum, default_value_t = GraphFormat::Tree)]
        format: GraphFormat,
        /// Shorthand for --format dot.
        #[arg(long, conflicts_with = "format")]
        dot: bool,
        /// Shorthand for --format json.
        #[arg(long, conflicts_with_all = ["format", "dot"])]
        json: bool,
        /// Levels to show (text and DOT).
        #[arg(long, default_value_t = 99)]
        depth: usize,
    },
    /// The same document on every engine this machine has: one table of total, preamble,
    /// body and page times. Runs each engine twice by default so font-cache builds on the
    /// first run do not skew the result.
    Compare {
        /// The document.
        main: PathBuf,
        /// Engines to compare (default: all three; the ones not installed are listed as such).
        #[arg(long, value_enum, value_delimiter = ',')]
        engines: Vec<EngineCmd>,
        /// Drive each build with latexmk.
        #[arg(long)]
        latexmk: bool,
        /// Runs per engine; the last is reported.
        #[arg(long, default_value_t = 2)]
        runs: u32,
        /// Cache directory (default: `compare` under the document's cache directory; `prumo where`).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// Fonts loaded (TFMs read) vs fonts embedded in the PDF, with Type 3 / bitmap warnings
    /// and which unit loaded each family.
    Fonts {
        #[command(flatten)]
        build: BuildArgs,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// A corpus-wide keep list and a `tlmgr remove` plan (dry run unless --apply --yes).
    Prune {
        /// Documents: files, directories or glob patterns (a document is a .tex with \documentclass).
        #[arg(required = true)]
        paths: Vec<String>,
        /// Engines every document runs on (a `% !TEX program =` magic comment overrides).
        #[arg(long, value_enum, value_delimiter = ',', default_values_t = [EngineCmd::Pdflatex])]
        engines: Vec<EngineCmd>,
        /// Drive each build with latexmk (and keep the latexmk package).
        #[arg(long)]
        latexmk: bool,
        /// Path to texlive.tlpdb (default: the installed one).
        #[arg(long)]
        tlpdb: Option<PathBuf>,
        /// Extra packages to keep.
        #[arg(long, value_delimiter = ',')]
        keep: Vec<String>,
        /// Keep every installed hyphen-* package, or only the languages the corpus loaded.
        #[arg(long, value_enum, default_value_t = prumo::prune::Hyphenation::All)]
        keep_hyphenation: prumo::prune::Hyphenation,
        /// Caches and outputs (default: `prune` under the cache directory; `prumo where`).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Run the plan with tlmgr. Recomputes and prints it first; requires --yes.
        #[arg(long)]
        apply: bool,
        /// Confirm --apply.
        #[arg(long, requires = "apply")]
        yes: bool,
        /// Machine-readable plan (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// Which \usepackage lines can go: remove each candidate's request, rebuild, compare
    /// (phase 2). With --corpus, MAIN is one of your own .cls/.sty files and the verdict
    /// is merged over every document that loads it.
    Unused {
        #[command(flatten)]
        build: BuildArgs,
        /// Judge MAIN (a .cls/.sty of yours) against these documents: files, directories or
        /// globs, comma-separated or repeated. A package it loads is removable only if no
        /// document needs it.
        #[arg(long, value_name = "DOCS", value_delimiter = ',')]
        corpus: Vec<String>,
        /// Parallel candidate builds (default: half the cores).
        #[arg(long)]
        jobs: Option<usize>,
        /// What to compare: page text (fast) or rendered pages too (~10× slower).
        #[arg(long, value_enum, default_value_t = prumo::unused::Compare::Text)]
        compare: prumo::unused::Compare,
        /// Also test side-effect packages (fontenc, hyperref, microtype, …).
        #[arg(long)]
        test_all: bool,
        /// Per-candidate build timeout in seconds (default: 10× the baseline, at least 30).
        #[arg(long, default_value_t = 0)]
        timeout: u64,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
    /// The TeX Live packages this document needs, with the file and LaTeX unit that
    /// caused each (needs a texlive.tlpdb; file-level only without one).
    Packages {
        #[command(flatten)]
        build: BuildArgs,
        /// Path to texlive.tlpdb (default: `$(kpsewhich -var-value TEXMFROOT)/tlpkg/texlive.tlpdb`).
        #[arg(long)]
        tlpdb: Option<PathBuf>,
        /// Machine-readable output (schema 1).
        #[arg(long)]
        json: bool,
    },
}

/// The fonts report of a cached run (the saved map subset, pdffonts output and .fd files).
fn fonts_report_of(out: &RunOutput, model: &BuildModel) -> anyhow::Result<prumo::fonts::FontReport> {
    let lines = prumo::fls::parse_file(&out.fls)?;
    let trees =
        prumo::fls::Trees::from_kpsewhich(&std::fs::read_to_string(out.dir.join("kpsewhich.txt")).unwrap_or_default());
    let files = prumo::fls::input_files(&lines, &trees);
    let map_text = std::fs::read_to_string(out.dir.join("fontmap.txt")).ok();
    let pdffonts = std::fs::read_to_string(out.dir.join("pdffonts.txt")).ok();
    let fd_texts: Vec<(String, String)> = std::fs::read_dir(out.dir.join("fd"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).ok()?)))
        .collect();
    Ok(prumo::fonts::report(
        &out.job,
        model,
        &prumo::fonts::FontSources {
            files: &files,
            map_text: map_text.as_deref(),
            pdffonts_text: pdffonts.as_deref(),
            fd_texts: &fd_texts,
            installer: prumo::doctor::Installer::detect(),
        },
    ))
}

/// Parse the cached records of a run into a model.
fn model_of(out: &RunOutput) -> anyhow::Result<BuildModel> {
    let recs =
        prumo::records::parse_file(&out.records).with_context(|| format!("reading {}", out.records.display()))?;
    let mut model = BuildModel::from_records(&recs, &out.records.display().to_string())?;
    if let Some(code) = out.info.exit_code.filter(|c| *c != 0) {
        model.warnings.insert(
            0,
            format!(
                "the build exited with code {code}; timings cover only what ran (see {})",
                prumo::doctor::host_path(&out.dir.join("build.out"))
            ),
        );
    }
    if let Ok(lines) = prumo::fls::parse_file(&out.fls)
        && let Some(n) = prumo::fls::font_database_built(&lines)
    {
        let culprit = model
            .unit_table()
            .into_iter()
            .next()
            .map(|r| {
                format!(
                    "; it sits in {}'s exclusive column ({:.0} ms)",
                    r.name,
                    prumo::scaled_to_secs(r.exclusive) * 1000.0
                )
            })
            .unwrap_or_default();
        model.warnings.push(format!(
            "luaotfload built its font name database during this build (cold cache: {n} font files read){culprit}. Build once more for a representative timing."
        ));
    }
    Ok(model)
}

impl BuildArgs {
    /// `--engine`, else the document's `% !TEX program =` line, else pdflatex; with where
    /// the choice came from, for the build message.
    fn engine(&self) -> (EngineCmd, &'static str) {
        match (self.engine, prumo::prune::magic_engine(&self.main)) {
            (Some(e), _) => (e, ""),
            (None, Some(e)) => (e, " (from the % !TEX program line)"),
            (None, None) => (EngineCmd::Pdflatex, " (the default; --engine or a % !TEX program line changes it)"),
        }
    }
}

fn run_options(b: &BuildArgs) -> RunOptions {
    RunOptions {
        latexmk: b.latexmk,
        build: b.build.clone(),
        out: b.out.clone(),
        ..RunOptions::new(b.main.clone(), b.engine().0)
    }
}

/// The cached run, or a fresh one.
fn ensure_run(b: &BuildArgs, quiet: bool) -> anyhow::Result<RunOutput> {
    let opts = run_options(b);
    if !b.rebuild
        && let Some(c) = run::cached(&b.main, b.out.as_deref())?
        && !run::is_stale(&c, &opts)
    {
        if !quiet {
            eprintln!("prumo: cached run in {}  (--rebuild to build again)", prumo::doctor::host_path(&c.dir));
        }
        return Ok(c);
    }
    if !quiet {
        let with = match &b.build {
            Some(cmd) => format!("`{cmd}`"),
            None => {
                let (engine, why) = b.engine();
                format!("{}{}{why}", engine.command(), if b.latexmk { " via latexmk" } else { "" })
            }
        };
        let where_ = prumo::doctor::image().map(|i| format!(" in the {i} image")).unwrap_or_default();
        eprintln!("prumo: building {} with {with}{where_}", b.main.display());
    }
    let out = run::run(&opts)?;
    if !quiet {
        eprintln!("prumo: artifacts in {}", prumo::doctor::host_path(&out.dir));
    }
    if let Some(code) = out.info.exit_code
        && code != 0
        && !quiet
    {
        eprintln!("prumo: the build exited with {code}; see {}", prumo::doctor::host_path(&out.dir.join("build.out")));
        if b.engine.is_none()
            && let Some(hint) = engine_hint(&out.log)
        {
            eprintln!("prumo: hint: {hint}");
        }
        if let Some(hint) = small_image_hint(&out.log) {
            eprintln!("prumo: hint: {hint}");
        }
        if let Some(hint) = font_hint(&out.log) {
            eprintln!("prumo: hint: {hint}");
        }
    }
    Ok(out)
}

/// When fontspec gives up on a font, name it. The log says `The font "X" cannot be found`
/// (wrapped over `(fontspec)` lines, the name stays on one). Inside an image, say so: the
/// container has TeX Live's fonts and the mounted directory, not the machine's fonts.
fn font_hint(log: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(log).ok()?;
    let name = missing_font(&text)?;
    Some(match prumo::doctor::image() {
        Some(image) => format!(
            "fontspec cannot find the font \"{name}\" in the {image} image, which has TeX Live's fonts and the mounted directory, not this machine's fonts"
        ),
        None => format!("fontspec cannot find the font \"{name}\" on this machine"),
    })
}

/// The font name in fontspec's `The font "X" cannot be found` message, if the log has one.
fn missing_font(log: &str) -> Option<&str> {
    const OPEN: &str = "The font \"";
    let rest = &log[log.find(OPEN)? + OPEN.len()..];
    let name = &rest[..rest.find('"')?];
    (rest[name.len()..].starts_with("\" cannot be found") && !name.contains('\n') && !name.is_empty()).then_some(name)
}

/// In the `small` image (scheme-small), a missing file or font usually means the package
/// is not in that scheme; the full image has it.
fn small_image_hint(log: &std::path::Path) -> Option<String> {
    let image = prumo::doctor::image()?;
    if !image.contains("small") {
        return None;
    }
    let text = std::fs::read_to_string(log).ok()?;
    let missing = text.contains("not found") || text.contains("Package fontspec Error");
    missing.then(|| {
        let full = image.replace("-small", "").replace(":small", "");
        format!("this is the small image and the log has a missing file or font; the full image has every TeX Live package: PRUMO_IMAGE={full}")
    })
}

/// When a defaulted pdflatex fails on something only another engine has, name it.
fn engine_hint(log: &std::path::Path) -> Option<&'static str> {
    let text = std::fs::read_to_string(log).ok()?;
    if text.contains("\\directlua") || text.contains("LuaTeX is required") {
        Some(
            "the log mentions \\directlua; this document wants --engine lualatex (or a `% !TEX program = lualatex` line in its first 20 lines)",
        )
    } else if text.contains("requires either XeTeX or LuaTeX") || text.contains("fontspec") {
        Some(
            "the log mentions fontspec; this document wants --engine xelatex or lualatex (or a `% !TEX program = ...` line in its first 20 lines)",
        )
    } else {
        None
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor { json, check_log: Some(main), out } => {
            let Some(run) = run::cached(&main, out.as_deref())? else {
                anyhow::bail!("no cached run for {}: run `prumo run {}` first", main.display(), main.display());
            };
            let model = model_of(&run)?;
            let lines = prumo::fls::parse_file(&run.fls)?;
            let trees = prumo::fls::Trees::from_kpsewhich(
                &std::fs::read_to_string(run.dir.join("kpsewhich.txt")).unwrap_or_default(),
            );
            let files = prumo::fls::input_files(&lines, &trees);
            let log = prumo::texlog::parse_file(&run.log).ok();
            let c = prumo::doctor::hook_coverage(&model, &files, log.as_ref());
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                print!("{}", prumo::doctor::coverage_text(&run.job, &c, log.is_some()));
            }
        }
        Cmd::Doctor { json, check_log: None, .. } => {
            let report = prumo::doctor::run();
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render_text());
            }
            if !report.ready {
                std::process::exit(1);
            }
        }
        Cmd::Clean { main, stale, yes, json } => {
            let root = run::cache_root();
            let mut listing = prumo::cache::list(&root);
            if let Some(main) = &main {
                let mine = run::default_out(main)?;
                listing.entries.retain(|e| e.dir == mine);
            } else if stale {
                listing.entries.retain(|e| e.stale());
            }
            listing.bytes = listing.entries.iter().map(|e| e.bytes).sum();
            if json {
                println!("{}", serde_json::to_string_pretty(&listing)?);
            } else {
                print!("{}", prumo::cache::text(&listing, prumo::doctor::host_path));
            }
            if listing.entries.is_empty() {
                if !json {
                    println!("nothing to remove");
                }
            } else if yes {
                let (n, bytes) = prumo::cache::remove(&listing.entries)?;
                println!("removed {n} entr{} ({})", if n == 1 { "y" } else { "ies" }, prumo::cache::human_bytes(bytes));
            } else if !json {
                let stale_n = listing.entries.iter().filter(|e| e.stale()).count();
                println!(
                    "nothing removed: --yes deletes {} ({}){}",
                    if listing.entries.len() == 1 {
                        "this entry".to_string()
                    } else {
                        format!("these {} entries", listing.entries.len())
                    },
                    prumo::cache::human_bytes(listing.bytes),
                    if stale_n > 0 && !stale && main.is_none() {
                        format!("; --stale picks the {stale_n} whose document is gone")
                    } else {
                        String::new()
                    }
                );
            }
        }
        Cmd::Where { main: None, .. } => {
            println!("{}", prumo::doctor::host_path(&run::cache_root()));
        }
        Cmd::Where { main: Some(main), out } => {
            let dir = run::cache_dir(&main, out.as_deref())?;
            let state = match run::cached(&main, out.as_deref())? {
                Some(c) => format!("cached run of {}", c.info.command.join(" ")),
                None => "no cached run".to_string(),
            };
            println!("{}  ({state})", prumo::doctor::host_path(&dir));
        }
        Cmd::Run { build } => {
            let out = run::run(&run_options(&build))?;
            println!("prumo: artifacts in {}", prumo::doctor::host_path(&out.dir));
            println!("  command: {}", out.info.command.join(" "));
            println!("  exit code: {}", out.info.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into()));
        }
        Cmd::Time { build, by, top, depth, json, plain, preamble, html } => {
            let out = ensure_run(&build, json)?;
            let model = model_of(&out)?;
            let files_read = prumo::fls::parse_file(&out.fls).ok().map(|l| prumo::fls::inputs(&l).len());
            if let Some(html) = html {
                let path = html.unwrap_or_else(|| PathBuf::from(format!("{}-prumo.html", out.job)));
                let fonts = fonts_report_of(&out, &model).ok();
                let page = prumo::html::page(&model, &out.job, files_read, fonts.as_ref());
                std::fs::write(&path, page).with_context(|| format!("writing {}", path.display()))?;
                let shown = std::fs::canonicalize(&path).unwrap_or(path);
                println!("prumo: wrote {}", prumo::doctor::host_path(&shown));
            } else if json {
                println!("{}", serde_json::to_string_pretty(&report::time_json(&model, &out.job))?);
            } else {
                let by = match by {
                    By::Unit => TimeBy::Unit,
                    By::Package => TimeBy::Package,
                    By::File => TimeBy::File,
                    By::Page => TimeBy::Page,
                };
                print!(
                    "{}",
                    report::time_text(
                        &model,
                        &out.job,
                        files_read,
                        by,
                        top,
                        depth,
                        prumo::chart::detect(plain).map(|l| prumo::chart::Look { preamble, ..l })
                    )
                );
            }
        }
        Cmd::Graph { build, format, dot, json, depth } => {
            let format = if dot {
                GraphFormat::Dot
            } else if json {
                GraphFormat::Json
            } else {
                format
            };
            let out = ensure_run(&build, !matches!(format, GraphFormat::Tree))?;
            let model = model_of(&out)?;
            match format {
                GraphFormat::Tree => print!("{}", prumo::graph::tree_text(&model, depth)),
                GraphFormat::Dot => print!("{}", prumo::graph::dot(&model, &out.job, depth)),
                GraphFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&prumo::graph::graph_json(&model, &out.job))?)
                }
            }
        }
        Cmd::Unused { build, corpus, jobs, compare, test_all, timeout, json } => {
            let jobs =
                jobs.unwrap_or_else(|| (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2) / 2).max(1));
            let opts = prumo::unused::UnusedOptions {
                main: build.main.clone(),
                engine: build.engine().0,
                latexmk: build.latexmk,
                build: build.build.clone(),
                jobs,
                compare,
                test_all,
                timeout_secs: timeout,
                out: build.out.clone(),
                env: vec![],
            };
            let progress = |m: &str| {
                if !json {
                    eprintln!("prumo: {m}");
                }
            };
            if corpus.is_empty() {
                let r = prumo::unused::run(&opts, progress)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                } else {
                    print!("{}", prumo::unused::text(&r));
                }
            } else {
                let r = prumo::unused::run_corpus(&build.main, &corpus, &opts, progress)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                } else {
                    print!("{}", prumo::unused::corpus_text(&r));
                }
            }
        }
        Cmd::Prune { paths, engines, latexmk, tlpdb, keep, keep_hyphenation, out, apply, yes, json } => {
            let out = out.unwrap_or_else(|| run::cache_root().join("prune"));
            if !json {
                eprintln!("prumo: caches and outputs in {}", prumo::doctor::host_path(&out));
            }
            let tlpdb_path = tlpdb.or_else(prumo::doctor::tlpdb_path).ok_or_else(|| anyhow::anyhow!("no texlive.tlpdb found: prune needs a vanilla TeX Live (Debian/Ubuntu TeX Live has no package database); pass --tlpdb"))?;
            let db =
                prumo::tlpdb::parse_file(&tlpdb_path).with_context(|| format!("reading {}", tlpdb_path.display()))?;
            let opts = prumo::prune::PruneOptions {
                paths,
                engines,
                latexmk,
                keep,
                keep_hyphenation,
                out: out.clone(),
                env: vec![],
            };
            let runs = prumo::prune::run_corpus(&opts, &db, |m| {
                if !json {
                    eprintln!("prumo: {m}");
                }
            })?;
            let plan = prumo::prune::plan(&db, &runs, &opts);
            prumo::prune::write_outputs(&plan, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                print!("{}", prumo::prune::text(&plan, &out));
            }
            if apply {
                let r = prumo::prune::apply(&plan, &out, yes, |m| eprintln!("prumo: {m}"))?;
                println!(
                    "\napplied: {} tlmgr commands, {} packages removed, {} refused; log in {}",
                    r.commands_run,
                    r.removed,
                    r.refused.len(),
                    prumo::doctor::host_path(&r.log)
                );
                if !r.refused.is_empty() {
                    println!("  refused by tlmgr (still needed by something installed): {}", r.refused.join(" "));
                    std::process::exit(1);
                }
            }
        }
        Cmd::Compare { main, engines, latexmk, runs, out, json } => {
            let engines = if engines.is_empty() { prumo::compare::ALL_ENGINES.to_vec() } else { engines };
            let c = prumo::compare::compare(
                &prumo::compare::CompareOptions { main, engines, latexmk, runs, out, env: vec![] },
                |msg| {
                    if !json {
                        eprintln!("prumo: {msg}");
                    }
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                print!("{}", prumo::compare::text(&c));
            }
        }
        Cmd::Fonts { build, json } => {
            let out = ensure_run(&build, json)?;
            let model = model_of(&out)?;
            let r = fonts_report_of(&out, &model)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", prumo::fonts::text(&r));
            }
        }
        Cmd::Packages { build, tlpdb, json } => {
            let out = ensure_run(&build, json)?;
            let model = model_of(&out)?;
            let lines = prumo::fls::parse_file(&out.fls)?;
            let trees = prumo::fls::Trees::from_kpsewhich(
                &std::fs::read_to_string(out.dir.join("kpsewhich.txt")).unwrap_or_default(),
            );
            let files = prumo::fls::input_files(&lines, &trees);
            let tlpdb_path = tlpdb.or_else(prumo::doctor::tlpdb_path);
            match tlpdb_path {
                Some(p) => {
                    let db = prumo::tlpdb::parse_file(&p).with_context(|| format!("reading {}", p.display()))?;
                    let r = prumo::packages::resolve(&out.job, &model, &files, &db, out.info.engine.command());
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r)?);
                    } else {
                        print!("{}", prumo::packages::text(&r));
                    }
                }
                None if json => {
                    anyhow::bail!("no texlive.tlpdb found; pass --tlpdb or use the text output (file-level only)")
                }
                None => print!("{}", prumo::packages::file_level_text(&out.job, &files)),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::missing_font;

    #[test]
    fn fontspec_font_name_is_read_from_the_log() {
        let log = "! Package fontspec Error: \n(fontspec)                The font \"IranNastaliq-Web\" cannot be found; this\n(fontspec)                may be but usually is not a fontspec bug.\n";
        assert_eq!(missing_font(log), Some("IranNastaliq-Web"));
        assert_eq!(missing_font("(fontspec) Font family 'lmroman10-regular.otf(0)' created\n"), None);
        assert_eq!(missing_font("The font \"x\" is fine\n"), None);
    }
}
