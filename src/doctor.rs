//! `prumo doctor`: what this machine can do. Engines, kernel date, `-recorder`, Poppler,
//! kpathsea trees, tlpdb. Prints a table; the exit code says whether `prumo run` can work.
//!
//! Everything here shells out to tools on `PATH`; nothing touches the network.

use crate::fls::{InputFile, InputKind};
use crate::model::{BuildModel, FrameKind};
use crate::texlog::TexLog;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The oldest kernel prumo supports (Q5, decided 2026-11-08): hooks arrived 2020-10-01 and
/// raw package options (`\@raw@opt@`, the `opts` records `unused` needs) 2021-06-01. One
/// floor, no degraded mode: Debian bookworm ships 2022, TeX Live 2020 users are rare.
pub(crate) const MIN_KERNEL: &str = "2021-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Missing,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    /// True when at least one LaTeX engine, a new enough kernel and `-recorder` are there.
    pub ready: bool,
    /// True when a `texlive.tlpdb` was found: `packages`/`prune` can name tlmgr packages.
    pub tlpdb: Option<PathBuf>,
}

/// Where a vanilla TeX Live keeps its package database: `TEXMFROOT/tlpkg/texlive.tlpdb`.
fn tlpdb_candidate() -> PathBuf {
    let root = kpsewhich(&["-var-value", "TEXMFROOT"]).unwrap_or_default();
    Path::new(&root).join("tlpkg/texlive.tlpdb")
}

/// The installation's `texlive.tlpdb`, when there is one (Debian's TeX Live has none).
/// One `kpsewhich` call, where `run()` would spawn every tool.
pub fn tlpdb_path() -> Option<PathBuf> {
    let db = tlpdb_candidate();
    db.is_file().then_some(db)
}

/// The TeX distribution's package manager on `PATH`, for the wording of install hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installer {
    /// `tlmgr` is on `PATH`.
    TexLive,
    /// MiKTeX (`initexmf` or `miktex` on `PATH`), which has no `tlmgr`.
    MikTex,
    /// Neither: a distribution's TeX Live without `tlmgr` (Debian), or no TeX at all.
    Unknown,
}

impl Installer {
    pub fn detect() -> Self {
        if which("initexmf").is_some() || which("miktex").is_some() {
            Self::MikTex
        } else if which("tlmgr").is_some() {
            Self::TexLive
        } else {
            Self::Unknown
        }
    }

    /// How to install `pkg` here, phrased for a hint.
    pub fn install(self, pkg: &str) -> String {
        match self {
            Self::TexLive => format!("`tlmgr install {pkg}`"),
            Self::MikTex => format!("`miktex packages install {pkg}`"),
            Self::Unknown => format!("install {pkg} with your TeX distribution's package manager"),
        }
    }
}

/// Find `cmd` on `PATH`. On Windows the file is `cmd.exe` (or another `PATHEXT` suffix).
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .map(|p| p.split(';').filter(|e| !e.is_empty()).map(str::to_ascii_lowercase).collect())
            .unwrap_or_else(|_| vec![".exe".into(), ".cmd".into(), ".bat".into()])
    } else {
        Vec::new()
    };
    std::env::split_paths(&path).find_map(|d| {
        let bare = d.join(cmd);
        if bare.is_file() {
            return Some(bare);
        }
        exts.iter().map(|e| d.join(format!("{cmd}{e}"))).find(|p| p.is_file())
    })
}

/// First line of `cmd arg`'s stdout (stderr for tools that print versions there).
fn first_line(cmd: &str, arg: &str) -> Option<String> {
    let out = Command::new(cmd).arg(arg).output().ok()?;
    let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
    String::from_utf8_lossy(&text).lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

/// `kpsewhich -var-value NAME`, or `kpsewhich FILE` when `name` has an extension.
fn kpsewhich(args: &[&str]) -> Option<String> {
    let out = Command::new("kpsewhich").args(args).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Read `\edef\fmtversion {YYYY-MM-DD}` from `latex.ltx` (the braces may be on the next line).
pub(crate) fn kernel_date_from_ltx(text: &str) -> Option<String> {
    let i = text.find("\\fmtversion")?;
    let rest = text[i + "\\fmtversion".len()..].trim_start();
    let rest = rest.strip_prefix('{')?;
    let end = rest.find('}')?;
    let date = rest[..end].trim();
    (date.len() == 10 && date.as_bytes()[4] == b'-').then(|| date.to_string())
}

fn engine_check(name: &str, binary: &str) -> (Check, bool) {
    match which(name) {
        Some(p) => {
            let v = first_line(name, "--version").unwrap_or_default();
            (Check { name: name.to_string(), status: Status::Ok, detail: format!("{v}  ({})", p.display()) }, true)
        }
        None => (
            Check {
                name: name.to_string(),
                status: Status::Missing,
                detail: format!("not on PATH (engine: {binary})"),
            },
            false,
        ),
    }
}

/// The name of the container image prumo runs in, when the image says so (`PRUMO_IMAGE`,
/// set by `docker/Dockerfile.user`). Otherwise prumo cannot tell and says nothing.
pub fn image() -> Option<String> {
    std::env::var("PRUMO_IMAGE").ok().filter(|s| !s.is_empty())
}

/// A path as the user should read it. Inside a container prumo sees the mount points
/// while the user sees the directories they mounted; `PRUMO_PATH_MAP`
/// (`/doc=/home/kevin/thesis;/cache=/home/kevin/.cache/prumo`; `docker/prumo` mounts at
/// the same paths and says so with `/x=/x` pairs) translates. With an image but no map,
/// the path is marked as the container's.
pub fn host_path(p: &Path) -> String {
    host_path_with(p, std::env::var("PRUMO_PATH_MAP").ok().as_deref(), image().is_some())
}

/// The `inside=host` pairs of a `PRUMO_PATH_MAP`, in order.
fn path_map(map: Option<&str>) -> Vec<(&str, &str)> {
    map.unwrap_or_default().split(';').filter_map(|pair| pair.split_once('=')).filter(|(i, _)| !i.is_empty()).collect()
}

fn host_path_with(p: &Path, map: Option<&str>, in_image: bool) -> String {
    for (inside, host) in path_map(map) {
        if let Ok(rest) = p.strip_prefix(inside) {
            let rest = rest.to_string_lossy();
            if rest.is_empty() {
                return host.to_string();
            }
            // A Windows host (`C:\thesis`, `\\server\share`) wants its own separators;
            // prumo runs on Linux inside the image, so `Path::join` would mix them.
            // Built by hand rather than `Path::join`, so the separators are the host's
            // whatever prumo itself runs on.
            let windows = host.as_bytes().get(1) == Some(&b':') || host.starts_with("\\\\");
            return if windows {
                format!("{}\\{}", host.trim_end_matches('\\'), rest.replace('/', "\\"))
            } else {
                format!("{}/{}", host.trim_end_matches('/'), rest.replace('\\', "/"))
            };
        }
    }
    let shown = plain(&p.display().to_string());
    if in_image { format!("{shown} (inside the image)") } else { shown }
}

/// A path without Windows' verbatim prefix (`\\?\C:\x` is what `canonicalize` returns
/// there; nobody types it).
fn plain(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
    }
}

/// What the image can see of the user's machine, for the doctor's "image" row.
fn mounts_text(map: Option<&str>) -> String {
    let pairs = path_map(map);
    if pairs.is_empty() {
        return "printed paths are the container's (docker/prumo mounts your directories at their own paths)".into();
    }
    let same: Vec<&str> = pairs.iter().filter(|(i, h)| i == h).map(|(_, h)| *h).collect();
    let other: Vec<String> = pairs.iter().filter(|(i, h)| i != h).map(|(i, h)| format!("{i} is {h}")).collect();
    let mut parts = Vec::new();
    if !same.is_empty() {
        parts.push(format!("{} mounted at the same path", same.join(" and ")));
    }
    parts.extend(other);
    parts.join("; ")
}

/// A tool on `PATH` is ok with its version line; otherwise a warning that says `absent`.
fn tool_check(tool: &str, version_arg: &str, absent: &str) -> Check {
    match which(tool) {
        Some(_) => {
            Check { name: tool.into(), status: Status::Ok, detail: first_line(tool, version_arg).unwrap_or_default() }
        }
        None => Check { name: tool.into(), status: Status::Warn, detail: absent.into() },
    }
}

/// Inside a container image: say so, and what the image can see of the machine.
fn image_check() -> Option<Check> {
    image().map(|img| Check {
        name: "image".into(),
        status: Status::Ok,
        detail: format!(
            "{img}: the engines and TeX Live below are the image's, not this machine's; {}",
            mounts_text(std::env::var("PRUMO_PATH_MAP").ok().as_deref())
        ),
    })
}

/// The cache directory, what it holds, and the command that empties it.
fn cache_check() -> Check {
    let root = crate::run::cache_root();
    let cache = crate::cache::list(&root);
    Check {
        name: "cache".into(),
        status: Status::Ok,
        detail: format!(
            "{}  ({} entr{}, {}; prumo clean)",
            host_path(&root),
            cache.entries.len(),
            if cache.entries.len() == 1 { "y" } else { "ies" },
            crate::cache::human_bytes(cache.bytes)
        ),
    }
}

/// The kernel date from `latex.ltx` against `MIN_KERNEL`. The flag says it is new enough.
fn kernel_check() -> (Check, bool) {
    let ltx = kpsewhich(&["latex.ltx"]).map(PathBuf::from);
    let date = ltx.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| kernel_date_from_ltx(&t));
    let name = || "LaTeX kernel".to_string();
    match (&ltx, &date) {
        (Some(p), Some(d)) if d.as_str() >= MIN_KERNEL => {
            (Check { name: name(), status: Status::Ok, detail: format!("{d}  ({})", p.display()) }, true)
        }
        (Some(p), Some(d)) => (
            Check {
                name: name(),
                status: Status::Missing,
                detail: format!(
                    "{d} is older than {MIN_KERNEL}, the oldest kernel prumo supports (hook system and raw package options)  ({})",
                    p.display()
                ),
            },
            false,
        ),
        (Some(p), None) => (
            Check {
                name: name(),
                status: Status::Missing,
                detail: format!("could not read \\fmtversion from {}", p.display()),
            },
            false,
        ),
        (None, _) => {
            (Check { name: name(), status: Status::Missing, detail: "kpsewhich latex.ltx found nothing".into() }, false)
        }
    }
}

/// `-recorder`: every TeX Live engine since 2008 has it; ask the one prumo would use.
fn recorder_check() -> (Check, bool) {
    let recorder = ["pdftex", "xetex", "luahbtex", "luatex"]
        .iter()
        .find(|b| which(b).is_some())
        .map(|b| (b, Command::new(b).arg("--help").output().ok()));
    let name = || "-recorder".to_string();
    match &recorder {
        Some((b, Some(out))) if String::from_utf8_lossy(&out.stdout).contains("-recorder") => {
            (Check { name: name(), status: Status::Ok, detail: format!("{b} --help lists it") }, true)
        }
        Some((b, _)) => {
            (Check { name: name(), status: Status::Missing, detail: format!("{b} --help does not list it") }, false)
        }
        None => (Check { name: name(), status: Status::Missing, detail: "no engine binary to ask".into() }, false),
    }
}

/// The kpathsea tree and the tlpdb: `TEXMFDIST` and `texlive.tlpdb` rows, or one
/// `kpsewhich` row when there is no TeX at all. The path is the tlpdb when it exists.
fn tlpdb_checks() -> (Vec<Check>, Option<PathBuf>) {
    let Some(dist) = kpsewhich(&["-var-value", "TEXMFDIST"]) else {
        let none = Check {
            name: "kpsewhich".into(),
            status: Status::Missing,
            detail: "not on PATH; no TeX installation found".into(),
        };
        return (vec![none], None);
    };
    let mut checks = vec![Check { name: "TEXMFDIST".into(), status: Status::Ok, detail: dist }];
    let db = tlpdb_candidate();
    match std::fs::metadata(&db) {
        Ok(m) => {
            checks.push(Check {
                name: "texlive.tlpdb".into(),
                status: Status::Ok,
                detail: format!("{} ({} MB); packages: full", db.display(), m.len() / 1_000_000),
            });
            (checks, Some(db))
        }
        Err(_) => {
            checks.push(Check {
                name: "texlive.tlpdb".into(),
                status: Status::Warn,
                detail: format!(
                    "not at {}; packages: file-level only, prune unavailable (Debian/Ubuntu TeX Live?)",
                    db.display()
                ),
            });
            (checks, None)
        }
    }
}

pub fn run() -> Report {
    let mut checks: Vec<Check> = image_check().into_iter().collect();
    checks.push(cache_check());
    let mut engines = 0;
    for (name, bin) in [("pdflatex", "pdftex"), ("xelatex", "xetex"), ("lualatex", "luahbtex")] {
        let (c, ok) = engine_check(name, bin);
        engines += ok as u32;
        checks.push(c);
    }
    checks.push(tool_check("latexmk", "--version", "not on PATH; --latexmk unavailable"));
    let (kernel, kernel_ok) = kernel_check();
    checks.push(kernel);
    let (recorder, recorder_ok) = recorder_check();
    checks.push(recorder);
    for tool in ["pdffonts", "pdftotext", "pdfinfo", "pdftoppm"] {
        checks.push(tool_check(tool, "-v", "not on PATH (poppler-utils); font and text comparisons degrade"));
    }
    let (tl, tlpdb) = tlpdb_checks();
    checks.extend(tl);
    checks.push(tool_check("tlmgr", "--version", "not on PATH; prune --apply unavailable"));
    Report { ready: engines > 0 && kernel_ok && recorder_ok, tlpdb, checks }
}

impl Report {
    pub fn render_text(&self) -> String {
        let mut s = String::new();
        let w = self.checks.iter().map(|c| c.name.len()).max().unwrap_or(8);
        for c in &self.checks {
            let mark = match c.status {
                Status::Ok => "ok  ",
                Status::Warn => "warn",
                Status::Missing => "MISSING",
            };
            s.push_str(&format!("{mark:<8}{:<w$}  {}\n", c.name, c.detail));
        }
        s.push_str(if self.ready {
            "\nready: prumo run can instrument a build here\n"
        } else {
            "\nnot ready: see MISSING rows\n"
        });
        s
    }
}

// `prumo doctor --check-log`: the records cross-checked against the .fls and the log.

/// A file the `.fls` proves was read but no `file+` record saw: loaded with the TeX
/// primitive `\input` (the kernel reads `.aux` that way), or by something that bypasses
/// the LaTeX file machinery. Its position in the load tree is unknown.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Outside {
    pub path: PathBuf,
    pub kind: InputKind,
}

/// What the records cover, cross-checked against the `.fls` and the log.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Coverage {
    pub schema: u32,
    /// Macro-level files from the `.fls` with no `file+` record (fonts, maps, formats and
    /// the probe itself are not counted: they are never `\input`).
    pub outside_hooks: Vec<Outside>,
    /// Files the log shows opened that no record saw (by base name).
    pub log_only: Vec<String>,
    /// Files a record saw that the log parser did not find: a parser miss, worth a look.
    pub hooked_not_in_log: Vec<String>,
    pub log_errors: Vec<String>,
    pub log_warnings: usize,
    pub log_pages: usize,
}

/// Cross-check the records' file frames against the `.fls` inputs and (if given) the log.
pub fn hook_coverage(model: &BuildModel, files: &[InputFile], log: Option<&TexLog>) -> Coverage {
    let mut hooked: BTreeSet<String> = BTreeSet::new();
    model.root.walk(&mut |fr, _| {
        let base = fr.name.rsplit('/').next().unwrap_or(&fr.name).to_string();
        match fr.kind {
            FrameKind::Pkg => hooked.insert(format!("{base}.sty")),
            FrameKind::Cls => hooked.insert(format!("{base}.cls")),
            _ => hooked.insert(base),
        };
    });
    let mut c = Coverage { schema: crate::JSON_SCHEMA, ..Default::default() };
    for f in files {
        let macro_level = matches!(
            f.kind,
            InputKind::Tex
                | InputKind::Sty
                | InputKind::Cls
                | InputKind::Clo
                | InputKind::Fd
                | InputKind::Def
                | InputKind::Cfg
                | InputKind::Ltx
                | InputKind::Ldf
                | InputKind::Other
        );
        let base = f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Kernel file substitution (atbegshi.sty → atbegshi-ltx.sty): the requested file is
        // opened for the existence check, so the .fls lists it, but the substitute is read.
        let substituted =
            base.strip_suffix(".sty").map(|s| format!("{s}-ltx.sty")).is_some_and(|s| hooked.contains(&s));
        if macro_level && base != "prumo.sty" && !hooked.contains(&base) && !substituted && f.path.extension().is_some()
        {
            c.outside_hooks.push(Outside { path: f.path.clone(), kind: f.kind });
        }
    }
    if let Some(log) = log {
        let logged: BTreeSet<String> =
            log.files.iter().map(|f| f.path.rsplit('/').next().unwrap_or(&f.path).to_string()).collect();
        c.log_only = logged.iter().filter(|n| !hooked.contains(*n) && n.as_str() != "prumo.sty").cloned().collect();
        // `.sty`/`.cls` keys above are derived from unit names; only bare file frames are checked here.
        c.hooked_not_in_log = hooked
            .iter()
            .filter(|n| !logged.contains(*n) && !n.ends_with(".sty") && !n.ends_with(".cls"))
            .cloned()
            .collect();
        c.log_errors = log.errors.clone();
        c.log_warnings = log.warnings.len();
        c.log_pages = log.pages;
    }
    c
}

/// Text for `prumo doctor --check-log`.
pub fn coverage_text(job: &str, c: &Coverage, has_log: bool) -> String {
    use std::fmt::Write as _;
    let mut s = format!("== hook coverage: {job}\n");
    if c.outside_hooks.is_empty() {
        let _ = writeln!(s, "  every macro-level file in the .fls has a file record");
    } else {
        let _ = writeln!(s, "  read outside the LaTeX file hooks (position in the load tree unknown):");
        for o in &c.outside_hooks {
            let note = match o.kind {
                InputKind::Aux | InputKind::AuxLike => "; read by the kernel with the \\input primitive",
                InputKind::Tex => "; probably \\input<space>file or \\@@input",
                _ => "",
            };
            let _ = writeln!(s, "    {}  ({:?}{note})", o.path.display(), o.kind);
        }
    }
    if has_log {
        let _ = writeln!(
            s,
            "  log vs records: {} hooked files missing from the log{}; {} log files never hooked{}",
            c.hooked_not_in_log.len(),
            list(&c.hooked_not_in_log),
            c.log_only.len(),
            list(&c.log_only)
        );
        let _ = writeln!(s, "  log: {} errors, {} warnings, {} pages", c.log_errors.len(), c.log_warnings, c.log_pages);
        for e in &c.log_errors {
            let _ = writeln!(s, "    ! {e}");
        }
    }
    s
}

fn list(v: &[String]) -> String {
    if v.is_empty() { String::new() } else { format!(" ({})", v.join(", ")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_date_parses_both_layouts() {
        assert_eq!(kernel_date_from_ltx("\\edef\\fmtversion\n   {2026-11-01}\n"), Some("2026-11-01".into()));
        assert_eq!(kernel_date_from_ltx("\\def\\fmtversion{2020-02-02}"), Some("2020-02-02".into()));
        assert_eq!(kernel_date_from_ltx("\\fmtversion is mentioned here"), None);
        assert_eq!(kernel_date_from_ltx("\\fmtversion{nope}"), None);
    }

    #[test]
    fn host_paths_follow_the_map() {
        let map = Some("/doc=/home/k/thesis;/cache=/home/k/.cache/prumo");
        assert_eq!(host_path_with(Path::new("/doc/main-prumo.html"), map, true), "/home/k/thesis/main-prumo.html");
        assert_eq!(host_path_with(Path::new("/cache/main-ab/main"), map, true), "/home/k/.cache/prumo/main-ab/main");
        assert_eq!(host_path_with(Path::new("/doc"), map, true), "/home/k/thesis");
        assert_eq!(host_path_with(Path::new("/tmp/x"), map, true), "/tmp/x (inside the image)");
        assert_eq!(host_path_with(Path::new("/doc/a"), None, true), "/doc/a (inside the image)");
        assert_eq!(host_path_with(Path::new("/doc/a"), None, false), "/doc/a");
        assert_eq!(host_path_with(Path::new("/doc/a"), Some("garbage"), false), "/doc/a");
        assert_eq!(plain("\\\\?\\C:\\Users\\k\\x.html"), "C:\\Users\\k\\x.html");
        assert_eq!(plain("\\\\?\\UNC\\srv\\share\\x"), "\\\\srv\\share\\x");
        assert_eq!(plain("/home/k/x"), "/home/k/x");
        let same = Some("/home/k/thesis=/home/k/thesis;/home/k/.cache/prumo=/home/k/.cache/prumo");
        assert_eq!(host_path_with(Path::new("/home/k/thesis/x.tex"), same, true), "/home/k/thesis/x.tex");
        assert_eq!(mounts_text(same), "/home/k/thesis and /home/k/.cache/prumo mounted at the same path");
        assert_eq!(mounts_text(map), "/doc is /home/k/thesis; /cache is /home/k/.cache/prumo");
        assert!(mounts_text(None).starts_with("printed paths are the container's"));
        // docker/prumo.cmd: C:\Users\k\thesis is mounted at /C/Users/k/thesis.
        let win = Some(
            "/C/Users/k/thesis=C:\\Users\\k\\thesis;/C/Users/k/AppData/Local/prumo=C:\\Users\\k\\AppData\\Local\\prumo",
        );
        assert_eq!(
            host_path_with(Path::new("/C/Users/k/thesis/main-prumo.html"), win, true),
            "C:\\Users\\k\\thesis\\main-prumo.html"
        );
        assert_eq!(
            host_path_with(Path::new("/C/Users/k/AppData/Local/prumo/x-1/x/run.json"), win, true),
            "C:\\Users\\k\\AppData\\Local\\prumo\\x-1\\x\\run.json"
        );
        assert_eq!(host_path_with(Path::new("/C/Users/k/thesis"), win, true), "C:\\Users\\k\\thesis");
        assert_eq!(
            mounts_text(win),
            "/C/Users/k/thesis is C:\\Users\\k\\thesis; /C/Users/k/AppData/Local/prumo is C:\\Users\\k\\AppData\\Local\\prumo"
        );
    }

    #[test]
    fn report_runs_without_tex() {
        // Whatever the machine has, run() must not panic and must render.
        let r = run();
        assert!(r.checks.len() >= 12, "{:?}", r.checks.iter().map(|c| &c.name).collect::<Vec<_>>());
        assert!(r.render_text().contains("LaTeX kernel"));
    }
}
