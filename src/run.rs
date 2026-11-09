//! Running the instrumented build.
//!
//! Rule 1 of the charter: the user's files are never touched. The document's directory is
//! copied to a temp dir, the probe is written next to it and prepended to `TEXINPUTS`, the
//! engine runs on the copy, and the artifacts (`.prumo`, `.fls`, `.log`, `.pdf`, `pdffonts`
//! output, `kpsewhich.txt`, `run.json`) are copied into the document's directory under the
//! cache root (`prumo where`, `PRUMO_CACHE`). Nothing is written next to the document.

use crate::fls::{self, KPSEWHICH_VARS, Tree, Trees};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// The probe, embedded so users never install it.
pub const PROBE: &str = include_str!("../tex/prumo.sty");

/// Which `latex` command to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum EngineCmd {
    Pdflatex,
    Xelatex,
    Lualatex,
}

impl EngineCmd {
    pub fn command(self) -> &'static str {
        match self {
            EngineCmd::Pdflatex => "pdflatex",
            EngineCmd::Xelatex => "xelatex",
            EngineCmd::Lualatex => "lualatex",
        }
    }

    /// The latexmk switch that selects this engine.
    pub fn latexmk_flag(self) -> &'static str {
        match self {
            EngineCmd::Pdflatex => "-pdf",
            EngineCmd::Xelatex => "-xelatex",
            EngineCmd::Lualatex => "-lualatex",
        }
    }
}

/// The command prumo wraps: the standard engine or latexmk invocation, or the user's own
/// (`--build "latexmk -pdf -shell-escape -outdir=build"`). prumo adds `-recorder`,
/// `-jobname` (unless the command has one), `-interaction=nonstopmode` (same) and the probe;
/// everything else in the command is the user's business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCmd {
    /// Program and flags; the document, if the command named it, is removed.
    pub argv: Vec<String>,
    pub latexmk: bool,
    /// The engine, when the command says: `pdflatex …`, `latexmk -xelatex …`. A bare
    /// `latexmk` leaves it to `latexmkrc`; the `start` record then says which engine ran.
    pub engine: Option<EngineCmd>,
    /// `-output-directory` / `-outdir`, relative to the project: where the PDF goes.
    pub out_dir: Option<PathBuf>,
    /// latexmk `-auxdir`: where `.fls`, `.log` and the records go; otherwise `out_dir`.
    pub aux_dir: Option<PathBuf>,
    /// A `-jobname` in the command; artifacts are named after it.
    pub jobname: Option<String>,
    /// The command as the user gave it; `None` for the standard invocation. Part of the
    /// cache key.
    pub source: Option<String>,
}

impl BuildCmd {
    /// `<engine> …` or `latexmk <engine flag> …`.
    pub fn standard(engine: EngineCmd, latexmk: bool) -> BuildCmd {
        let argv = if latexmk {
            vec!["latexmk".to_string(), engine.latexmk_flag().to_string()]
        } else {
            vec![engine.command().to_string()]
        };
        BuildCmd { argv, latexmk, engine: Some(engine), out_dir: None, aux_dir: None, jobname: None, source: None }
    }

    /// Parse a user command. `stem` is the document's file stem, so a token naming the
    /// document (`main`, `main.tex`, `./main.tex`) can be dropped: prumo passes the document
    /// itself, wrapped in the probe.
    pub fn parse(command: &str, stem: &str) -> Result<BuildCmd, RunError> {
        let bad = |msg: String| RunError::BadBuild { command: command.to_string(), msg };
        let words = split_words(command).map_err(|e| bad(e.to_string()))?;
        let Some(program) = words.first() else { return Err(bad("empty command".into())) };
        let tool = Path::new(program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tool = tool.strip_suffix(".exe").unwrap_or(&tool).to_string();
        let (latexmk, mut engine) = match tool.as_str() {
            "latexmk" => (true, None),
            "pdflatex" => (false, Some(EngineCmd::Pdflatex)),
            "xelatex" => (false, Some(EngineCmd::Xelatex)),
            "lualatex" => (false, Some(EngineCmd::Lualatex)),
            other => {
                return Err(bad(format!("prumo wraps pdflatex, xelatex, lualatex or latexmk, not {other}")));
            }
        };
        let mut cmd = BuildCmd {
            argv: vec![program.clone()],
            latexmk,
            engine: None,
            out_dir: None,
            aux_dir: None,
            jobname: None,
            source: Some(command.to_string()),
        };
        // Options that take a value, by the name web2c/latexmk know them under. web2c accepts
        // any unambiguous prefix of a long option (`-output-d`), so match on prefixes.
        fn valued(name: &str) -> Option<&'static str> {
            let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
            if is("output-directory", 8) || name == "outdir" {
                Some("out")
            } else if is("aux-directory", 5) || name == "auxdir" {
                Some("aux")
            } else if is("jobname", 4) {
                Some("job")
            } else {
                None
            }
        }
        let mut i = 1;
        while i < words.len() {
            let w = &words[i];
            let name = w.trim_start_matches('-');
            let is_flag = w.starts_with('-') && !name.is_empty();
            if latexmk && is_flag {
                engine = match name {
                    "pdf" => Some(EngineCmd::Pdflatex),
                    "xelatex" | "pdfxe" => Some(EngineCmd::Xelatex),
                    "lualatex" | "pdflua" => Some(EngineCmd::Lualatex),
                    "dvi" | "ps" | "pdfdvi" | "pdfps" => None,
                    _ => engine,
                };
            }
            let (key, value) = match name.split_once('=') {
                Some((k, v)) if is_flag => (k, Some(v.to_string())),
                _ if is_flag => (name, None),
                _ => ("", None),
            };
            match valued(key) {
                Some(which) => {
                    let value = match value {
                        Some(v) => v,
                        None => {
                            i += 1;
                            words.get(i).cloned().ok_or_else(|| bad(format!("{w} needs a value")))?
                        }
                    };
                    match which {
                        "out" => cmd.out_dir = Some(PathBuf::from(&value)),
                        "aux" => cmd.aux_dir = Some(PathBuf::from(&value)),
                        _ => cmd.jobname = Some(value.clone()),
                    }
                    cmd.argv.push(format!("-{key}={value}"));
                }
                None if !is_flag && is_document_token(w, stem) => {}
                None => cmd.argv.push(w.clone()),
            }
            i += 1;
        }
        cmd.engine = engine;
        Ok(cmd)
    }

    /// The program to look up on `PATH`.
    pub fn tool(&self) -> &str {
        &self.argv[0]
    }

    fn has_flag(&self, name: &str) -> bool {
        self.argv[1..].iter().any(|a| {
            let a = a.trim_start_matches('-');
            a == name || a.starts_with(&format!("{name}="))
        })
    }
}

fn is_document_token(w: &str, stem: &str) -> bool {
    let p = Path::new(w);
    let named = p.file_stem().is_some_and(|s| s == stem) && p.extension().is_none_or(|e| e == "tex");
    named && p.parent().is_none_or(|d| d.as_os_str().is_empty() || d == Path::new("."))
}

/// Split a command line the way a POSIX shell would (single and double quotes,
/// backslash escapes) without running a shell, so the probe argument reaches the engine
/// verbatim.
pub(crate) fn split_words(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err("unterminated single quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => cur.push(e),
                            Some(e) => {
                                cur.push('\\');
                                cur.push(e);
                            }
                            None => return Err("unterminated double quote".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("unterminated double quote".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(e) => cur.push(e),
                    None => return Err("trailing backslash".into()),
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// The document; its parent directory is the project.
    pub main: PathBuf,
    pub engine: EngineCmd,
    /// Drive the build with `latexmk` (multiple passes, biber, …) instead of one engine call.
    pub latexmk: bool,
    /// The user's own build command; overrides `engine` and `latexmk` (`BuildCmd::parse`).
    pub build: Option<String>,
    /// Where artifacts go; default `default_out(main)` in the user's cache directory.
    pub out: Option<PathBuf>,
    /// Extra environment for the engine (tests set `SOURCE_DATE_EPOCH`).
    pub env: Vec<(String, String)>,
    /// TeX code run before the probe and `\input{<job>}` (the unused pass blocks packages
    /// with it). Must be a single line with no `%`.
    pub pretex: Option<String>,
    /// Files to rewrite in the temp copy before the build, relative to the project (the
    /// unused pass removes a `\usepackage` request this way). The user's files are never
    /// touched.
    pub edits: Vec<(PathBuf, String)>,
    /// Kill the build (and its children) after this long; `RunError::TimedOut`.
    pub timeout: Option<std::time::Duration>,
}

impl RunOptions {
    /// The ordinary probed build of `main` with `engine`: no latexmk, no custom command,
    /// the default cache directory, no extra environment, no pretex, no edits, no timeout.
    pub fn new(main: PathBuf, engine: EngineCmd) -> Self {
        Self {
            main,
            engine,
            latexmk: false,
            build: None,
            out: None,
            env: vec![],
            pretex: None,
            edits: vec![],
            timeout: None,
        }
    }
}

/// What `run.json` records about a build.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunInfo {
    pub schema: u32,
    pub main: PathBuf,
    pub job: String,
    pub engine: EngineCmd,
    pub latexmk: bool,
    /// The user's `--build` command, if one drove this run.
    #[serde(default)]
    pub build: Option<String>,
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    /// Nanoseconds since the Unix epoch when the build started; a source modified after
    /// this makes the cache stale.
    pub started_at_ns: u64,
    /// The container image the build ran in (`PRUMO_IMAGE`), if any. A run from one image
    /// is not reused in another: they are different TeX Lives.
    #[serde(default)]
    pub image: Option<String>,
}

/// Artifacts of one build, all inside the cache directory.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub dir: PathBuf,
    pub job: String,
    pub info: RunInfo,
    pub records: PathBuf,
    pub fls: PathBuf,
    pub log: PathBuf,
    pub pdf: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{path}: not a file{hint}")]
    NotAFile { path: PathBuf, hint: &'static str },
    #[error("{0}: file name is not <job>.tex")]
    NotTex(PathBuf),
    #[error("{tool} is not on PATH (run `prumo doctor`)")]
    ToolMissing { tool: String },
    #[error("cannot {what} {path}: {source}")]
    Io {
        what: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the build wrote no {job}.prumo: the probe never ran (exit code {code:?}). Log tail:\n{tail}")]
    NoRecords { job: String, code: Option<i32>, tail: String },
    #[error("the build wrote no {job}.fls: -recorder was not honored (exit code {code:?})")]
    NoFls { job: String, code: Option<i32> },
    #[error(transparent)]
    Records(#[from] crate::records::RecordsError),
    #[error(transparent)]
    Fls(#[from] crate::fls::FlsError),
    #[error(transparent)]
    Model(#[from] crate::model::ModelError),
    #[error("--build {command:?}: {msg}")]
    BadBuild { command: String, msg: String },
    #[error("the build was killed after {secs:.0} s (timeout)")]
    TimedOut { secs: f64 },
    #[error("the build ran {engine}, which prumo does not support (pdfTeX, XeTeX, LuaTeX)")]
    UnsupportedEngine { engine: String },
    #[error("{path}: {source}")]
    BadRunJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

fn io<'a>(what: &'static str, path: &'a Path) -> impl FnOnce(std::io::Error) -> RunError + 'a {
    move |source| RunError::Io { what, path: path.to_path_buf(), source }
}

/// `<job>` and the project directory for a document path.
pub(crate) fn job_of(main: &Path) -> Result<(String, PathBuf), RunError> {
    if !main.is_file() {
        // In the image the container sees only the directory prumo was run from.
        let hint = if crate::doctor::image().is_some() {
            " (the image sees only the directory prumo was run from; run it from the document's directory)"
        } else {
            ""
        };
        return Err(RunError::NotAFile { path: main.to_path_buf(), hint });
    }
    let job = match (main.file_stem(), main.extension()) {
        (Some(stem), Some(ext)) if ext == "tex" => stem.to_string_lossy().into_owned(),
        _ => return Err(RunError::NotTex(main.to_path_buf())),
    };
    let abs = std::fs::canonicalize(main).map_err(io("resolve", main))?;
    let project = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
    Ok((job, project))
}

/// Where caches go when `--out` is not given (decided 2026-11-08: nothing lands next to the
/// document): `$PRUMO_CACHE`, else the platform cache directory plus `prumo`:
/// `$XDG_CACHE_HOME` or `~/.cache` on Linux, `~/Library/Caches` on macOS, `%LOCALAPPDATA%`
/// on Windows. Every command prints the directory it used.
pub fn cache_root() -> PathBuf {
    cache_root_with(|k| std::env::var_os(k))
}

fn cache_root_with(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
    let get = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = get("PRUMO_CACHE") {
        return p;
    }
    let home = get("HOME").or_else(|| get("USERPROFILE"));
    let base = if cfg!(target_os = "windows") {
        get("LOCALAPPDATA").or_else(|| home.map(|h| h.join("AppData").join("Local")))
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Caches"))
    } else {
        get("XDG_CACHE_HOME").or_else(|| home.map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("prumo")
}

/// A short stable key for an absolute path (FNV-1a, so it does not change with the Rust
/// version the way `DefaultHasher` may).
pub(crate) fn path_key(path: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_os_str().as_encoded_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:010x}", h >> 24)
}

/// The default `--out` for a document: `<cache_root>/<job>-<key of its absolute path>`, so
/// two `main.tex` in different directories never share a cache and the directory still
/// says which document it is for.
pub fn default_out(main: &Path) -> Result<PathBuf, RunError> {
    let (job, project) = job_of(main)?;
    let abs = project.join(main.file_name().unwrap_or_default());
    Ok(cache_root().join(format!("{job}-{}", path_key(&abs))))
}

/// The cache directory for a document: `<out>/<job>/`, with `default_out` for `out`.
pub fn cache_dir(main: &Path, out: Option<&Path>) -> Result<PathBuf, RunError> {
    let (job, _) = job_of(main)?;
    let out = match out {
        Some(o) => o.to_path_buf(),
        None => default_out(main)?,
    };
    Ok(out.join(job))
}

/// Copy a project tree, skipping `.prumo`, VCS directories and `target`.
fn copy_tree(from: &Path, to: &Path) -> Result<(), RunError> {
    std::fs::create_dir_all(to).map_err(io("create", to))?;
    for entry in std::fs::read_dir(from).map_err(io("read", from))? {
        let entry = entry.map_err(io("read", from))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // VCS and build directories, and `.prumo` from versions that wrote next to the document.
        if name_str == ".prumo" || name_str == ".git" || name_str == ".hg" || name_str == ".svn" || name_str == "target"
        {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        let ty = entry.file_type().map_err(io("stat", &src))?;
        if ty.is_dir() {
            copy_tree(&src, &dst)?;
        } else if ty.is_file() {
            // Big files (images, data) are hard-linked when the filesystem allows: TeX only
            // reads them. Small ones are copied, because TeX rewrites .aux/.toc/.out in place.
            let big = entry.metadata().map(|m| m.len() > 1 << 20).unwrap_or(false);
            if !(big && std::fs::hard_link(&src, &dst).is_ok()) {
                std::fs::copy(&src, &dst).map_err(io("copy", &src))?;
            }
        }
        // Symlinks are skipped: following them could leave the project; TeX will say if it misses one.
    }
    Ok(())
}

/// Run one build command in `dir` on `<stem>.tex`, with or without the probe. Returns the
/// argv and the exit code. Low-level: `run()` is the entry point; tests use this for the
/// invariance check.
pub fn build_once(
    dir: &Path,
    stem: &str,
    build: &BuildCmd,
    probe_dir: Option<&Path>,
    env: &[(String, String)],
    pretex: Option<&str>,
) -> Result<(Vec<String>, Option<i32>), RunError> {
    build_once_timed(dir, stem, build, probe_dir, env, pretex, None)
}

/// `build_once` with a timeout: the build runs in its own process group and the whole
/// group is killed when the time is up (latexmk's engine child included).
pub(crate) fn build_once_timed(
    dir: &Path,
    stem: &str,
    build: &BuildCmd,
    probe_dir: Option<&Path>,
    env: &[(String, String)],
    pretex: Option<&str>,
    timeout: Option<std::time::Duration>,
) -> Result<(Vec<String>, Option<i32>), RunError> {
    let tool = build.tool();
    let has_dir = Path::new(tool).components().count() > 1;
    let found = if has_dir { Path::new(tool).is_file() } else { crate::doctor::which(tool).is_some() };
    if !found {
        return Err(RunError::ToolMissing { tool: tool.into() });
    }
    let mut cmd = Command::new(tool);
    let mut argv: Vec<String> = build.argv.clone();
    let pre = match (pretex, probe_dir.is_some()) {
        (Some(p), true) => Some(format!("{p}\\RequirePackage{{prumo}}")),
        (Some(p), false) => Some(p.to_string()),
        (None, true) => Some("\\RequirePackage{prumo}".to_string()),
        (None, false) => None,
    };
    if !build.has_flag("interaction") {
        argv.push("-interaction=nonstopmode".into());
    }
    if !build.has_flag("recorder") {
        argv.push("-recorder".into());
    }
    if build.jobname.is_none() {
        argv.push(format!("-jobname={stem}"));
    }
    if build.latexmk {
        if let Some(p) = &pre {
            argv.push(format!("-usepretex={p}"));
        }
        argv.push(format!("{stem}.tex"));
    } else {
        argv.push(match &pre {
            Some(p) => format!("{p}\\input{{{stem}}}"),
            None => format!("{stem}.tex"),
        });
    }
    cmd.args(&argv[1..]).current_dir(dir);
    if let Some(p) = probe_dir {
        // kpathsea separates search paths with `:`, except on Windows where it is `;`.
        // A trailing separator keeps the default search path.
        const SEP: char = if cfg!(windows) { ';' } else { ':' };
        let existing = std::env::var("TEXINPUTS").unwrap_or_default();
        let tail = if existing.is_empty() || existing.ends_with(SEP) { String::new() } else { SEP.to_string() };
        cmd.env("TEXINPUTS", format!("{}{SEP}{existing}{tail}", p.display()));
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out_path = dir.join("build.out");
    let file = std::fs::File::create(&out_path).map_err(io("create", &out_path))?;
    let err = file.try_clone().map_err(io("create", &out_path))?;
    cmd.stdin(std::process::Stdio::null()).stdout(file).stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    let run_err = |e: std::io::Error| RunError::Io { what: "run", path: PathBuf::from(tool), source: e };
    let mut child = cmd.spawn().map_err(run_err)?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(run_err)? {
            return Ok((argv, status.code()));
        }
        if let Some(t) = timeout
            && started.elapsed() > t
        {
            #[cfg(unix)]
            let _ = Command::new("kill").args(["-9", &format!("-{}", child.id())]).output();
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::TimedOut { secs: started.elapsed().as_secs_f64() });
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The build command an options set asks for.
pub(crate) fn build_cmd(opts: &RunOptions) -> Result<BuildCmd, RunError> {
    match &opts.build {
        Some(b) => BuildCmd::parse(b, &job_of(&opts.main)?.0),
        None => Ok(BuildCmd::standard(opts.engine, opts.latexmk)),
    }
}

/// The temp copy a build runs in: the probe beside it, the project copied, edits applied.
struct Staged {
    probe_dir: PathBuf,
    work: PathBuf,
}

fn stage(tmp: &Path, project: &Path, edits: &[(PathBuf, String)]) -> Result<Staged, RunError> {
    let probe_dir = tmp.join("probe");
    std::fs::create_dir_all(&probe_dir).map_err(io("create", &probe_dir))?;
    std::fs::write(probe_dir.join("prumo.sty"), PROBE).map_err(io("write", &probe_dir.join("prumo.sty")))?;
    let work = tmp.join("project");
    copy_tree(project, &work)?;
    for (rel, text) in edits {
        let p = work.join(rel);
        std::fs::write(&p, text).map_err(io("write", &p))?;
    }
    Ok(Staged { probe_dir, work })
}

/// What a build left behind, copied into the cache directory.
struct Artifacts {
    records: PathBuf,
    fls: PathBuf,
    log: PathBuf,
    pdf: Option<PathBuf>,
}

/// Copy the job's artifacts out of the temp copy. The records file was checked before;
/// the `.fls` is required; the log and the PDF may be missing.
fn collect(
    aux_dir: &Path,
    pdf_dir: &Path,
    work: &Path,
    cache: &Path,
    job: &str,
    exit_code: Option<i32>,
) -> Result<Artifacts, RunError> {
    std::fs::create_dir_all(cache).map_err(io("create", cache))?;
    let copied = |from: &Path, ext: &str| -> Result<Option<PathBuf>, RunError> {
        let src = from.join(format!("{job}.{ext}"));
        if !src.is_file() {
            return Ok(None);
        }
        let dst = cache.join(format!("{job}.{ext}"));
        std::fs::copy(&src, &dst).map_err(io("copy", &src))?;
        Ok(Some(dst))
    };
    let records = copied(aux_dir, "prumo")?.expect("checked above");
    let fls = copied(aux_dir, "fls")?.ok_or_else(|| RunError::NoFls { job: job.to_string(), code: exit_code })?;
    let log = copied(aux_dir, "log")?.unwrap_or_else(|| cache.join(format!("{job}.log")));
    let pdf = copied(pdf_dir, "pdf")?;
    let _ = std::fs::copy(work.join("build.out"), cache.join("build.out"));
    Ok(Artifacts { records, fls, log, pdf })
}

/// The engine a bare `latexmk` ran, from the start record.
fn engine_of(records: &Path) -> Result<EngineCmd, RunError> {
    use crate::records::{Engine, Event};
    let recs = crate::records::parse_file(records)?;
    match recs.first().map(|r| &r.event) {
        Some(Event::Start { engine: Engine::PdfTex, .. }) => Ok(EngineCmd::Pdflatex),
        Some(Event::Start { engine: Engine::XeTex, .. }) => Ok(EngineCmd::Xelatex),
        Some(Event::Start { engine: Engine::LuaTex, .. }) => Ok(EngineCmd::Lualatex),
        Some(Event::Start { engine_name, .. }) => Err(RunError::UnsupportedEngine { engine: engine_name.clone() }),
        _ => Err(RunError::UnsupportedEngine { engine: "unknown".into() }),
    }
}

/// The sidecar files later commands read: `pdffonts` output, the kpathsea variables, and
/// the font sources the `.fls` names.
fn save_sidecars(cache: &Path, pdf: Option<&Path>, fls: &Path) {
    if let Some(pdf) = pdf
        && crate::doctor::which("pdffonts").is_some()
        && let Ok(out) = Command::new("pdffonts").arg(pdf).output()
    {
        let _ = std::fs::write(cache.join("pdffonts.txt"), out.stdout);
    }
    let _ = std::fs::write(cache.join("kpsewhich.txt"), kpsewhich_text());
    save_font_sources(cache, fls);
}

/// The instrumented build: copy, probe, run, collect.
pub fn run(opts: &RunOptions) -> Result<RunOutput, RunError> {
    let (stem, project) = job_of(&opts.main)?;
    let build = build_cmd(opts)?;
    // Artifacts are named after the job; `-jobname` in a user command renames them.
    let job = build.jobname.clone().unwrap_or_else(|| stem.clone());
    let cache = cache_dir(&opts.main, opts.out.as_deref())?;
    let started_at_ns = unix_ns(SystemTime::now());
    let tmp = tempfile::Builder::new().prefix("prumo-").tempdir().map_err(io("create temp dir", Path::new("/tmp")))?;
    let staged = stage(tmp.path(), &project, &opts.edits)?;
    let (command, exit_code) = build_once_timed(
        &staged.work,
        &stem,
        &build,
        Some(&staged.probe_dir),
        &opts.env,
        opts.pretex.as_deref(),
        opts.timeout,
    )?;

    // With -output-directory / -outdir the PDF lands there; latexmk's -auxdir takes the
    // .fls, .log and records (verified: the probe's \iow_open follows openout).
    let pdf_dir = build.out_dir.as_ref().map(|d| staged.work.join(d)).unwrap_or_else(|| staged.work.clone());
    let aux_dir = build.aux_dir.as_ref().map(|d| staged.work.join(d)).unwrap_or_else(|| pdf_dir.clone());
    if !aux_dir.join(format!("{job}.prumo")).is_file() {
        let tail = std::fs::read_to_string(aux_dir.join(format!("{job}.log")))
            .or_else(|_| std::fs::read_to_string(staged.work.join("build.out")))
            .map(|t| tail_lines(&t, 25))
            .unwrap_or_default();
        return Err(RunError::NoRecords { job, code: exit_code, tail });
    }
    let a = collect(&aux_dir, &pdf_dir, &staged.work, &cache, &job, exit_code)?;
    // A bare `latexmk` leaves the engine to latexmkrc; the start record says what ran.
    let engine = match build.engine {
        Some(e) => e,
        None => engine_of(&a.records)?,
    };
    save_sidecars(&cache, a.pdf.as_deref(), &a.fls);

    let info = RunInfo {
        schema: crate::JSON_SCHEMA,
        main: std::fs::canonicalize(&opts.main).unwrap_or_else(|_| opts.main.clone()),
        job: job.clone(),
        engine,
        latexmk: build.latexmk,
        build: build.source.clone(),
        command,
        exit_code,
        started_at_ns,
        image: crate::doctor::image(),
    };
    let info_path = cache.join("run.json");
    let text = serde_json::to_string_pretty(&info)
        .map_err(|source| RunError::BadRunJson { path: info_path.clone(), source })?;
    std::fs::write(&info_path, text).map_err(io("write", &cache.join("run.json")))?;
    Ok(RunOutput { dir: cache, job, info, records: a.records, fls: a.fls, log: a.log, pdf: a.pdf })
}

fn unix_ns(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos().min(u64::MAX as u128) as u64).unwrap_or(0)
}

/// Save what the font report needs and the user's machine may later lack: the `pdftex.map`
/// lines for the TFMs read and the PostScript names the PDF embeds (`fontmap.txt`), and the
/// `.fd` files read (`fd/`). Best effort: a missing map only costs name resolution.
fn save_font_sources(cache: &Path, fls_path: &Path) {
    let Ok(lines) = fls::parse_file(fls_path) else { return };
    let inputs = fls::inputs(&lines);
    let mut map_path = inputs.iter().find(|p| p.file_name().is_some_and(|n| n == "pdftex.map")).cloned();
    if map_path.is_none()
        && let Ok(out) = Command::new("kpsewhich").args(["--format=map", "pdftex.map"]).output()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            map_path = Some(PathBuf::from(p));
        }
    }
    let tfms: std::collections::HashSet<String> = inputs
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "tfm"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .collect();
    let ps: std::collections::HashSet<String> = std::fs::read_to_string(cache.join("pdffonts.txt"))
        .map(|t| crate::fonts::parse_pdffonts(&t).into_iter().map(|f| f.name.to_lowercase()).collect())
        .unwrap_or_default();
    if let Some(map) = map_path
        && let Ok(text) = std::fs::read_to_string(&map)
    {
        let subset: String = text
            .lines()
            .filter(|l| {
                let mut t = l.split_whitespace();
                let tfm = t.next().unwrap_or_default().to_lowercase();
                let psn = t.next().unwrap_or_default().to_lowercase();
                tfms.contains(&tfm) || ps.contains(&psn)
            })
            .map(|l| format!("{l}\n"))
            .collect();
        let _ = std::fs::write(cache.join("fontmap.txt"), subset);
    }
    let fd_dir = cache.join("fd");
    for p in inputs.iter().filter(|p| p.extension().is_some_and(|e| e == "fd")) {
        if let Some(name) = p.file_name()
            && std::fs::create_dir_all(&fd_dir).is_ok()
        {
            let _ = std::fs::copy(p, fd_dir.join(name));
        }
    }
}

/// Parse the records of a run.
pub(crate) fn parse_records(out: &RunOutput) -> Result<Vec<crate::records::Record>, RunError> {
    Ok(crate::records::parse_file(&out.records)?)
}

/// The build model of a run: its records parsed and validated.
pub fn model_of(out: &RunOutput) -> Result<crate::model::BuildModel, RunError> {
    let recs = parse_records(out)?;
    Ok(crate::model::BuildModel::from_records(&recs, &out.records.display().to_string())?)
}

/// The last `n` lines of `text`, for log tails in messages.
pub(crate) fn tail_lines(text: &str, n: usize) -> String {
    let skip = text.lines().count().saturating_sub(n);
    text.lines().skip(skip).collect::<Vec<_>>().join("\n")
}

/// `NAME=value` lines for the texmf roots of the installation on `PATH`.
pub(crate) fn kpsewhich_text() -> String {
    KPSEWHICH_VARS
        .iter()
        .map(|v| {
            let val = Command::new("kpsewhich")
                .args(["-var-value", v])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            format!("{v}={val}\n")
        })
        .collect()
}

/// Load the cached run for a document, if there is one.
pub fn cached(main: &Path, out: Option<&Path>) -> Result<Option<RunOutput>, RunError> {
    let dir = cache_dir(main, out)?;
    let info_path = dir.join("run.json");
    if !info_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&info_path).map_err(io("read", &info_path))?;
    let info: RunInfo =
        serde_json::from_str(&text).map_err(|source| RunError::BadRunJson { path: info_path.clone(), source })?;
    // Two documents called main.tex sharing one --out would otherwise read each other's run.
    if std::fs::canonicalize(main).ok().as_ref() != Some(&info.main) {
        return Ok(None);
    }
    let job = info.job.clone();
    let p = |ext: &str| dir.join(format!("{job}.{ext}"));
    if !p("prumo").is_file() || !p("fls").is_file() {
        return Ok(None);
    }
    let pdf = p("pdf").is_file().then(|| p("pdf"));
    Ok(Some(RunOutput { records: p("prumo"), fls: p("fls"), log: p("log"), pdf, dir, job, info }))
}

/// True when a project file the cached build read is newer than the cache, or the build
/// asked for (engine, latexmk, `--build` command) is not the one cached. The `.fls` was
/// recorded in the temp copy; its `PWD` maps back to the project.
pub fn is_stale(cached: &RunOutput, opts: &RunOptions) -> bool {
    let main = &opts.main;
    let info = &cached.info;
    // A failed build is never an answer worth repeating: the user is fixing something.
    if info.exit_code != Some(0) {
        return true;
    }
    if info.image != crate::doctor::image() {
        return true;
    }
    if info.build != opts.build {
        return true;
    }
    if opts.build.is_none() && (info.engine != opts.engine || info.latexmk != opts.latexmk) {
        return true;
    }
    let Ok((_, project)) = job_of(main) else { return true };
    let built = info.started_at_ns;
    let Ok(lines) = fls::parse_file(&cached.fls) else { return true };
    let Some(pwd) = fls::pwd(&lines).map(Path::to_path_buf) else { return true };
    let trees = Trees {
        project: Some(pwd.clone()),
        ..Trees::from_kpsewhich(&std::fs::read_to_string(cached.dir.join("kpsewhich.txt")).unwrap_or_default())
    };
    // Files the build itself wrote (.aux, .toc, .out, …) are not sources: the user's own
    // build after prumo's must not make the cache stale.
    let written: std::collections::HashSet<PathBuf> = fls::outputs(&lines).into_iter().collect();
    for f in fls::input_files(&lines, &trees) {
        if f.tree != Tree::Project || written.contains(&f.path) {
            continue;
        }
        let Ok(rel) = f.path.strip_prefix(&pwd) else { continue };
        // Files the build itself wrote (.aux, .out, …) are not sources; skip those not in the project.
        let real = project.join(rel);
        match std::fs::metadata(&real).and_then(|m| m.modified()) {
            Ok(mtime) if unix_ns(mtime) > built => return true,
            Ok(_) => {}
            Err(_) => continue,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_and_project_from_a_tex_path() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("thesis.tex");
        std::fs::write(&main, "x").unwrap();
        let (job, project) = job_of(&main).unwrap();
        assert_eq!(job, "thesis");
        assert_eq!(project, std::fs::canonicalize(dir.path()).unwrap());
        assert!(matches!(job_of(&dir.path().join("nope.tex")), Err(RunError::NotAFile { .. })));
        let other = dir.path().join("notes.md");
        std::fs::write(&other, "x").unwrap();
        assert!(matches!(job_of(&other), Err(RunError::NotTex(_))));
        let out = default_out(&main).unwrap();
        assert!(out.starts_with(cache_root()), "{}", out.display());
        let name = out.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("thesis-") && name.len() == "thesis-".len() + 10, "{name}");
        assert_eq!(cache_dir(&main, None).unwrap(), out.join("thesis"));
        assert_eq!(cache_dir(&main, Some(Path::new("/tmp/o"))).unwrap(), PathBuf::from("/tmp/o/thesis"));
        // Another thesis.tex elsewhere gets its own directory; nothing lands next to either.
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("thesis.tex"), "x").unwrap();
        assert_ne!(default_out(&other.path().join("thesis.tex")).unwrap(), out);
        assert!(!dir.path().join(".prumo").exists());
    }

    #[test]
    fn cache_root_follows_the_platform_and_the_override() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| std::ffi::OsString::from(v))
        };
        assert_eq!(cache_root_with(env(&[("PRUMO_CACHE", "/x"), ("HOME", "/h")])), PathBuf::from("/x"));
        assert_eq!(cache_root_with(env(&[("PRUMO_CACHE", ""), ("HOME", "/h"), ("XDG_CACHE_HOME", "/c")])), {
            if cfg!(target_os = "macos") {
                PathBuf::from("/h/Library/Caches/prumo")
            } else if cfg!(target_os = "windows") {
                PathBuf::from("/h/AppData/Local/prumo")
            } else {
                PathBuf::from("/c/prumo")
            }
        });
        if cfg!(target_os = "linux") {
            assert_eq!(cache_root_with(env(&[("HOME", "/h")])), PathBuf::from("/h/.cache/prumo"));
        }
        assert_eq!(path_key(Path::new("/a/main.tex")).len(), 10);
        assert_ne!(path_key(Path::new("/a/main.tex")), path_key(Path::new("/b/main.tex")));
    }

    #[test]
    fn copy_tree_skips_cache_and_vcs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("figs")).unwrap();
        std::fs::create_dir_all(src.join(".prumo/main")).unwrap();
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join("main.tex"), "a").unwrap();
        std::fs::write(src.join("figs/a.pdf"), "b").unwrap();
        std::fs::write(src.join(".prumo/main/run.json"), "c").unwrap();
        let dst = dir.path().join("dst");
        copy_tree(&src, &dst).unwrap();
        assert!(dst.join("main.tex").is_file());
        assert!(dst.join("figs/a.pdf").is_file());
        assert!(!dst.join(".prumo").exists());
        assert!(!dst.join(".git").exists());
    }

    #[test]
    fn split_words_like_a_shell() {
        assert_eq!(split_words("a  b\tc").unwrap(), ["a", "b", "c"]);
        assert_eq!(
            split_words(r#"latexmk -pdf "-e \"$x\"" 'it''s' a\ b"#).unwrap(),
            ["latexmk", "-pdf", "-e \"$x\"", "its", "a b"]
        );
        assert_eq!(split_words("").unwrap(), Vec::<String>::new());
        assert!(split_words("a 'b").is_err());
        assert!(split_words("a \"b").is_err());
    }

    #[test]
    fn build_command_parsing() {
        let c = BuildCmd::parse("latexmk -pdf -shell-escape -outdir=build main.tex", "main").unwrap();
        assert!(c.latexmk);
        assert_eq!(c.engine, Some(EngineCmd::Pdflatex));
        assert_eq!(c.out_dir.as_deref(), Some(Path::new("build")));
        assert_eq!(c.aux_dir, None);
        assert_eq!(c.argv, ["latexmk", "-pdf", "-shell-escape", "-outdir=build"], "the document is dropped");

        let c = BuildCmd::parse("latexmk -xelatex -auxdir aux -output-directory out ./main", "main").unwrap();
        assert_eq!(c.engine, Some(EngineCmd::Xelatex));
        assert_eq!(c.aux_dir.as_deref(), Some(Path::new("aux")));
        assert_eq!(c.out_dir.as_deref(), Some(Path::new("out")));
        assert_eq!(c.argv, ["latexmk", "-xelatex", "-auxdir=aux", "-output-directory=out"]);

        let c = BuildCmd::parse("latexmk", "main").unwrap();
        assert_eq!(c.engine, None, "latexmkrc decides; the start record tells");
        let c = BuildCmd::parse("latexmk -pdf -dvi", "main").unwrap();
        assert_eq!(c.engine, None);

        let c = BuildCmd::parse("/opt/tl/bin/lualatex -shell-escape --output-d=build -jobname=thesis", "main").unwrap();
        assert!(!c.latexmk);
        assert_eq!(c.engine, Some(EngineCmd::Lualatex));
        assert_eq!(c.jobname.as_deref(), Some("thesis"));
        assert_eq!(c.out_dir.as_deref(), Some(Path::new("build")));
        assert_eq!(c.argv, ["/opt/tl/bin/lualatex", "-shell-escape", "-output-d=build", "-jobname=thesis"]);
        assert!(c.has_flag("jobname") && !c.has_flag("interaction"));

        let c = BuildCmd::parse("pdflatex -interaction=batchmode other.tex", "main").unwrap();
        assert_eq!(c.argv, ["pdflatex", "-interaction=batchmode", "other.tex"], "another file is kept");
        assert!(c.has_flag("interaction"));

        let e = BuildCmd::parse("tectonic -X compile", "main").unwrap_err().to_string();
        assert!(e.contains("not tectonic"), "{e}");
        assert!(BuildCmd::parse("", "main").is_err());
        assert!(
            BuildCmd::parse("pdflatex -output-directory", "main").unwrap_err().to_string().contains("needs a value")
        );

        let s = BuildCmd::standard(EngineCmd::Xelatex, true);
        assert_eq!(s.argv, ["latexmk", "-xelatex"]);
        assert_eq!(s.source, None);
    }

    #[test]
    fn probe_is_embedded() {
        assert!(PROBE.contains("\\ProvidesPackage{prumo}"));
        assert!(PROBE.contains("shipout/after"));
    }
}
