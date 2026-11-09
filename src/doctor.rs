//! `prumo doctor`: what this machine can do. Engines, kernel date, `-recorder`, Poppler,
//! kpathsea trees, tlpdb. Prints a table; the exit code says whether `prumo run` can work.
//!
//! Everything here shells out to tools on `PATH`; nothing touches the network.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The oldest kernel prumo supports (Q5, decided 2026-11-08): hooks arrived 2020-10-01 and
/// raw package options (`\@raw@opt@`, the `opts` records `unused` needs) 2021-06-01. One
/// floor, no degraded mode: Debian bookworm ships 2022, TeX Live 2020 users are rare.
pub const MIN_KERNEL: &str = "2021-06-01";

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
pub fn kernel_date_from_ltx(text: &str) -> Option<String> {
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

pub fn run() -> Report {
    let mut checks = Vec::new();
    if let Some(img) = image() {
        checks.push(Check {
            name: "image".into(),
            status: Status::Ok,
            detail: format!(
                "{img}: the engines and TeX Live below are the image's, not this machine's; {}",
                mounts_text(std::env::var("PRUMO_PATH_MAP").ok().as_deref())
            ),
        });
    }
    let root = crate::run::cache_root();
    let cache = crate::cache::list(&root);
    checks.push(Check {
        name: "cache".into(),
        status: Status::Ok,
        detail: format!(
            "{}  ({} entr{}, {}; prumo clean)",
            host_path(&root),
            cache.entries.len(),
            if cache.entries.len() == 1 { "y" } else { "ies" },
            crate::cache::human_bytes(cache.bytes)
        ),
    });
    let mut engines = 0;
    for (name, bin) in [("pdflatex", "pdftex"), ("xelatex", "xetex"), ("lualatex", "luahbtex")] {
        let (c, ok) = engine_check(name, bin);
        engines += ok as u32;
        checks.push(c);
    }
    checks.push(match which("latexmk") {
        Some(_) => Check {
            name: "latexmk".into(),
            status: Status::Ok,
            detail: first_line("latexmk", "--version").unwrap_or_default(),
        },
        None => {
            Check { name: "latexmk".into(), status: Status::Warn, detail: "not on PATH; --latexmk unavailable".into() }
        }
    });

    // Kernel date from latex.ltx.
    let mut kernel_ok = false;
    let ltx = kpsewhich(&["latex.ltx"]).map(PathBuf::from);
    let date = ltx.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| kernel_date_from_ltx(&t));
    checks.push(match (&ltx, &date) {
        (Some(p), Some(d)) if d.as_str() >= MIN_KERNEL => {
            kernel_ok = true;
            Check { name: "LaTeX kernel".into(), status: Status::Ok, detail: format!("{d}  ({})", p.display()) }
        }
        (Some(p), Some(d)) => Check {
            name: "LaTeX kernel".into(),
            status: Status::Missing,
            detail: format!(
                "{d} is older than {MIN_KERNEL}, the oldest kernel prumo supports (hook system and raw package options)  ({})",
                p.display()
            ),
        },
        (Some(p), None) => Check {
            name: "LaTeX kernel".into(),
            status: Status::Missing,
            detail: format!("could not read \\fmtversion from {}", p.display()),
        },
        (None, _) => Check {
            name: "LaTeX kernel".into(),
            status: Status::Missing,
            detail: "kpsewhich latex.ltx found nothing".into(),
        },
    });

    // -recorder: every TeX Live engine since 2008 has it; check the one we would use.
    let recorder = ["pdftex", "xetex", "luahbtex", "luatex"]
        .iter()
        .find(|b| which(b).is_some())
        .map(|b| (b, Command::new(b).arg("--help").output().ok()));
    let recorder_ok = match &recorder {
        Some((b, Some(out))) if String::from_utf8_lossy(&out.stdout).contains("-recorder") => {
            checks.push(Check { name: "-recorder".into(), status: Status::Ok, detail: format!("{b} --help lists it") });
            true
        }
        Some((b, _)) => {
            checks.push(Check {
                name: "-recorder".into(),
                status: Status::Missing,
                detail: format!("{b} --help does not list it"),
            });
            false
        }
        None => {
            checks.push(Check {
                name: "-recorder".into(),
                status: Status::Missing,
                detail: "no engine binary to ask".into(),
            });
            false
        }
    };

    for tool in ["pdffonts", "pdftotext", "pdfinfo", "pdftoppm"] {
        checks.push(match which(tool) {
            Some(_) => {
                Check { name: tool.into(), status: Status::Ok, detail: first_line(tool, "-v").unwrap_or_default() }
            }
            None => Check {
                name: tool.into(),
                status: Status::Warn,
                detail: "not on PATH (poppler-utils); font and text comparisons degrade".into(),
            },
        });
    }

    // kpathsea trees and the tlpdb.
    let mut tlpdb = None;
    match kpsewhich(&["-var-value", "TEXMFDIST"]) {
        Some(dist) => {
            checks.push(Check { name: "TEXMFDIST".into(), status: Status::Ok, detail: dist });
            let db = tlpdb_candidate();
            checks.push(match std::fs::metadata(&db) {
                Ok(m) => {
                    tlpdb = Some(db.clone());
                    Check {
                        name: "texlive.tlpdb".into(),
                        status: Status::Ok,
                        detail: format!("{} ({} MB); packages: full", db.display(), m.len() / 1_000_000),
                    }
                }
                Err(_) => Check {
                    name: "texlive.tlpdb".into(),
                    status: Status::Warn,
                    detail: format!(
                        "not at {}; packages: file-level only, prune unavailable (Debian/Ubuntu TeX Live?)",
                        db.display()
                    ),
                },
            });
        }
        None => checks.push(Check {
            name: "kpsewhich".into(),
            status: Status::Missing,
            detail: "not on PATH; no TeX installation found".into(),
        }),
    }
    checks.push(match which("tlmgr") {
        Some(_) => Check {
            name: "tlmgr".into(),
            status: Status::Ok,
            detail: first_line("tlmgr", "--version").unwrap_or_default(),
        },
        None => Check {
            name: "tlmgr".into(),
            status: Status::Warn,
            detail: "not on PATH; prune --apply unavailable".into(),
        },
    });

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
