//! Parser for `<job>.fls`, the file list every engine writes under `-recorder`.
//!
//! Three line kinds: `PWD <dir>`, `INPUT <path>`, `OUTPUT <path>`. Relative paths are
//! relative to `PWD`. The same file is often listed several times (`t1pcr.fd` three times
//! in the basic fixture); `inputs()` deduplicates while keeping first-seen order.
//!
//! Classification into texmf trees needs the kpathsea variables of the installation that
//! ran the build (`kpsewhich -var-value TEXMFDIST` and friends). Tests inject them.

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// One line of the `.fls` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlsLine {
    Pwd(PathBuf),
    Input(PathBuf),
    Output(PathBuf),
}

/// A parse failure; names the file and line.
#[derive(Debug, thiserror::Error)]
pub enum FlsError {
    #[error("{file}:{line}: {msg}")]
    Line { file: String, line: usize, msg: String },
    #[error("{file}: cannot read: {source}")]
    Io {
        file: String,
        #[source]
        source: std::io::Error,
    },
}

/// Parse a `.fls` file from disk.
pub fn parse_file(path: &Path) -> Result<Vec<FlsLine>, FlsError> {
    let file = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|source| FlsError::Io { file: file.clone(), source })?;
    parse_str(&String::from_utf8_lossy(&bytes), &file)
}

/// `C:/…` or `c:\…`: a Windows drive path. TeX Live on Windows writes forward slashes with
/// the drive letter in either case; MiKTeX writes backslashes.
pub(crate) fn windows_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

/// One spelling for a Windows path: forward slashes, lowercase drive letter. POSIX paths
/// pass through untouched. `backslashes` says the file is from a Windows build, so a
/// relative `.\main.tex` is converted too.
pub(crate) fn portable(s: &str, backslashes: bool) -> PathBuf {
    let drive = windows_drive(s);
    if !(drive || backslashes || s.starts_with("\\\\")) {
        return PathBuf::from(s);
    }
    let mut t = s.replace('\\', "/");
    if drive {
        t[..1].make_ascii_lowercase();
    }
    PathBuf::from(t)
}

/// Absolute on the machine that wrote the `.fls`, whatever this one is.
fn is_absolute(p: &Path) -> bool {
    p.is_absolute() || windows_drive(&p.to_string_lossy())
}

/// Parse `.fls` text. Unknown line kinds are an error: the format has exactly three.
/// Windows paths (`C:/…`, `C:\…`, CRLF) are read into one portable spelling, so artifacts
/// from a Windows machine parse everywhere and classify by the same roots.
pub(crate) fn parse_str(text: &str, file: &str) -> Result<Vec<FlsLine>, FlsError> {
    let mut out = Vec::new();
    let windows = text.lines().any(|l| l.split_once(' ').is_some_and(|(_, rest)| windows_drive(rest.trim())));
    for (idx, raw) in text.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            continue;
        }
        let (kind, rest) = raw.split_once(' ').unwrap_or((raw, ""));
        let path = portable(rest.trim_end(), windows);
        let line = match kind {
            "PWD" => FlsLine::Pwd(path),
            "INPUT" => FlsLine::Input(path),
            "OUTPUT" => FlsLine::Output(path),
            other => {
                return Err(FlsError::Line {
                    file: file.to_string(),
                    line: idx + 1,
                    msg: format!("unknown line kind {other:?}"),
                });
            }
        };
        out.push(line);
    }
    Ok(out)
}

/// The `PWD` the recorder wrote, if any. Engines write it as the first line.
pub fn pwd(lines: &[FlsLine]) -> Option<&Path> {
    lines.iter().find_map(|l| match l {
        FlsLine::Pwd(p) => Some(p.as_path()),
        _ => None,
    })
}

/// Every `INPUT` path, resolved against `PWD`, lexically normalized, deduplicated,
/// first-seen order. No filesystem access: artifacts from another machine stay parseable.
pub fn inputs(lines: &[FlsLine]) -> Vec<PathBuf> {
    resolved(lines, |l| match l {
        FlsLine::Input(p) => Some(p),
        _ => None,
    })
}

/// Every `OUTPUT` path, resolved and normalized like `inputs()`: the files the build wrote.
pub fn outputs(lines: &[FlsLine]) -> Vec<PathBuf> {
    resolved(lines, |l| match l {
        FlsLine::Output(p) => Some(p),
        _ => None,
    })
}

/// luaotfload wrote its font name database during this build: the cache was cold, and the
/// engine scanned every font it could find before the first font selection. The time lands
/// in whichever file selected that font (the class's `size11.clo`, 4.8 s on florilegium's
/// manual), not in a frame of its own. `Some(n)`, with `n` the font files the build read.
pub fn font_database_built(lines: &[FlsLine]) -> Option<usize> {
    let built = outputs(lines)
        .iter()
        .any(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("luaotfload-names.")));
    built.then(|| {
        inputs(lines)
            .iter()
            .filter(|p| {
                p.extension().is_some_and(|e| {
                    matches!(e.to_string_lossy().to_ascii_lowercase().as_str(), "otf" | "ttf" | "ttc" | "pfb" | "afm")
                })
            })
            .count()
    })
}

fn resolved(lines: &[FlsLine], pick: impl Fn(&FlsLine) -> Option<&PathBuf>) -> Vec<PathBuf> {
    let pwd = pwd(lines).map(Path::to_path_buf).unwrap_or_default();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in lines.iter().filter_map(pick) {
        let abs = normalize(&if is_absolute(p) { p.clone() } else { pwd.join(p) });
        if seen.insert(abs.clone()) {
            out.push(abs);
        }
    }
    out
}

/// Lexical normalization: drop `.`, fold `..`, keep the root. Symlinks are left alone
/// (`canonicalize` touches the filesystem; `run.rs` does that when the files exist).
pub(crate) fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    // On Windows `push` joins with backslashes; the portable spelling is forward slashes,
    // the same as the roots from `kpsewhich`, or nothing would classify.
    if windows_drive(&p.to_string_lossy()) { portable(&out.to_string_lossy(), true) } else { out }
}

/// What a file is, by extension. Enough to separate fonts from macros from generated files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Tex,
    Sty,
    Cls,
    Clo,
    Fd,
    Def,
    Cfg,
    Ltx,
    Ldf,
    Lua,
    Tfm,
    Vf,
    Pfb,
    Pfa,
    Otf,
    Ttf,
    /// Bitmap font generated by `mktexpk`: extension like `.600pk`.
    Pk,
    Map,
    Enc,
    Fmt,
    Cnf,
    Aux,
    /// `.toc`, `.lof`, `.lot`, `.out`, `.nav`, `.bbl`, … files the document wrote last run.
    AuxLike,
    /// Data read by Lua or the engine, never `\input`: Unicode tables (`.txt`), font caches
    /// (`.luc`, `.gz`), `.dat`, `.ini`, `.tcx`.
    Data,
    /// Graphics the engine reads for `\includegraphics` and friends (beamer's navigation
    /// icons are `.pdf` files): `.pdf`, `.png`, `.jpg`, `.eps`, `.mps`, `.svg`.
    Image,
    Other,
}

impl InputKind {
    pub fn of(path: &Path) -> InputKind {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
        if ext.ends_with("pk") && ext[..ext.len() - 2].chars().all(|c| c.is_ascii_digit()) && ext.len() > 2 {
            return InputKind::Pk;
        }
        match ext.to_ascii_lowercase().as_str() {
            "tex" => InputKind::Tex,
            "sty" => InputKind::Sty,
            "cls" => InputKind::Cls,
            "clo" => InputKind::Clo,
            "fd" => InputKind::Fd,
            "def" => InputKind::Def,
            "cfg" => InputKind::Cfg,
            "ltx" => InputKind::Ltx,
            "ldf" => InputKind::Ldf,
            "lua" => InputKind::Lua,
            "tfm" => InputKind::Tfm,
            "vf" => InputKind::Vf,
            "pfb" => InputKind::Pfb,
            "pfa" => InputKind::Pfa,
            "otf" => InputKind::Otf,
            "ttf" => InputKind::Ttf,
            "map" => InputKind::Map,
            "enc" => InputKind::Enc,
            "fmt" => InputKind::Fmt,
            "cnf" => InputKind::Cnf,
            "aux" => InputKind::Aux,
            "txt" | "luc" | "gz" | "dat" | "ini" | "tcx" | "json" | "xml" | "lst" => InputKind::Data,
            "toc" | "lof" | "lot" | "out" | "nav" | "snm" | "bbl" | "ind" | "gls" | "idx" | "glo" => InputKind::AuxLike,
            "pdf" | "png" | "jpg" | "jpeg" | "eps" | "mps" | "svg" | "bmp" | "gif" | "jbig2" | "jp2" => {
                InputKind::Image
            }
            _ => InputKind::Other,
        }
    }

    /// True for font files of any flavor (metrics, outlines, bitmaps, virtual).
    pub fn is_font(self) -> bool {
        matches!(
            self,
            InputKind::Tfm
                | InputKind::Vf
                | InputKind::Pfb
                | InputKind::Pfa
                | InputKind::Otf
                | InputKind::Ttf
                | InputKind::Pk
        )
    }
}

/// Which texmf tree (or other place) a file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tree {
    /// `TEXMFDIST`: the distribution's files, the only ones a tlpdb maps to packages.
    TexmfDist,
    /// `TEXMFVAR` or `TEXMFSYSVAR`: generated files (formats, `.pk`, `pdftex.map`, `ls-R`).
    TexmfVar,
    /// `TEXMFCONFIG` or `TEXMFSYSCONFIG`: `updmap.cfg`, `fmtutil.cnf` edits.
    TexmfConfig,
    /// `TEXMFLOCAL`: admin additions.
    TexmfLocal,
    /// `TEXMFHOME`: user additions.
    TexmfHome,
    /// Under the build's `PWD`: the document and its own files.
    Project,
    /// Anything else (`/etc/texmf/web2c/texmf.cnf`, a path on `TEXINPUTS`).
    System,
}

impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Tree::TexmfDist => "texmf-dist",
            Tree::TexmfVar => "texmf-var",
            Tree::TexmfConfig => "texmf-config",
            Tree::TexmfLocal => "texmf-local",
            Tree::TexmfHome => "texmf-home",
            Tree::Project => "project",
            Tree::System => "system",
        })
    }
}

/// The roots that classify paths. Fill from `kpsewhich -var-value <NAME>`; every field is
/// optional because Debian lacks some and tests set only what they need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trees {
    pub texmf_dist: Option<PathBuf>,
    pub texmf_var: Option<PathBuf>,
    pub texmf_sysvar: Option<PathBuf>,
    pub texmf_config: Option<PathBuf>,
    pub texmf_sysconfig: Option<PathBuf>,
    pub texmf_local: Option<PathBuf>,
    pub texmf_home: Option<PathBuf>,
    /// The build directory (`PWD` from the `.fls`).
    pub project: Option<PathBuf>,
}

/// The kpathsea variables `Trees` is built from, in the order `kpsewhich.txt` lists them.
pub(crate) const KPSEWHICH_VARS: [&str; 8] =
    ["TEXMFROOT", "TEXMFDIST", "TEXMFVAR", "TEXMFSYSVAR", "TEXMFCONFIG", "TEXMFSYSCONFIG", "TEXMFLOCAL", "TEXMFHOME"];

impl Trees {
    /// Build from `NAME=value` lines (what `scripts/make_artifacts.sh` saves as
    /// `kpsewhich.txt`, and what `run.rs` gets from `kpsewhich -var-value`). Unknown names
    /// and empty values are ignored.
    pub fn from_kpsewhich(text: &str) -> Trees {
        let mut t = Trees::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            let p = Some(portable(v, false));
            match k.trim() {
                "TEXMFDIST" => t.texmf_dist = p,
                "TEXMFVAR" => t.texmf_var = p,
                "TEXMFSYSVAR" => t.texmf_sysvar = p,
                "TEXMFCONFIG" => t.texmf_config = p,
                "TEXMFSYSCONFIG" => t.texmf_sysconfig = p,
                "TEXMFLOCAL" => t.texmf_local = p,
                "TEXMFHOME" => t.texmf_home = p,
                _ => {}
            }
        }
        t
    }

    /// Longest matching root wins, so a project inside `$HOME` still beats `TEXMFHOME`
    /// when the project directory is the deeper one.
    pub fn classify(&self, path: &Path) -> Tree {
        let candidates = [
            (Tree::TexmfDist, &self.texmf_dist),
            (Tree::TexmfVar, &self.texmf_var),
            (Tree::TexmfVar, &self.texmf_sysvar),
            (Tree::TexmfConfig, &self.texmf_config),
            (Tree::TexmfConfig, &self.texmf_sysconfig),
            (Tree::TexmfLocal, &self.texmf_local),
            (Tree::TexmfHome, &self.texmf_home),
            (Tree::Project, &self.project),
        ];
        let mut best: Option<(usize, Tree)> = None;
        for (tree, root) in candidates {
            if let Some(root) = root
                && under(path, root)
            {
                let depth = root.components().count();
                if best.is_none_or(|(d, _)| depth > d) {
                    best = Some((depth, tree));
                }
            }
        }
        best.map(|(_, t)| t).unwrap_or(Tree::System)
    }
}

/// `path` is `root` or inside it. Windows roots compare case-insensitively: kpathsea and
/// the recorder do not agree on the case of `C:/TeXLive`.
fn under(path: &Path, root: &Path) -> bool {
    let r = root.to_string_lossy();
    if !windows_drive(&r) {
        return path.starts_with(root);
    }
    let p = path.to_string_lossy().to_lowercase();
    let r = r.to_lowercase();
    let r = r.trim_end_matches('/');
    p == r || p.strip_prefix(r).is_some_and(|rest| rest.starts_with('/'))
}

/// A deduplicated input with its classification.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InputFile {
    pub path: PathBuf,
    pub kind: InputKind,
    pub tree: Tree,
}

/// Classify every input of a parsed `.fls`.
pub fn input_files(lines: &[FlsLine], trees: &Trees) -> Vec<InputFile> {
    let mut trees = trees.clone();
    if trees.project.is_none() {
        trees.project = pwd(lines).map(normalize);
    }
    inputs(lines)
        .into_iter()
        .map(|path| InputFile { kind: InputKind::of(&path), tree: trees.classify(&path), path })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "PWD /tmp/build
INPUT /etc/texmf/web2c/texmf.cnf
INPUT /usr/share/texlive/texmf-dist/web2c/texmf.cnf
INPUT /var/lib/texmf/web2c/pdftex/pdflatex.fmt
INPUT /home/kevin/prumo/tex/prumo.sty
OUTPUT main.log
INPUT ./main.tex
INPUT ./main.tex
INPUT /usr/share/texlive/texmf-dist/tex/latex/base/article.cls
INPUT /usr/share/texlive/texmf-dist/fonts/tfm/adobe/courier/pcrr8t.tfm
INPUT /usr/share/texlive/texmf-dist/tex/latex/psnfss/t1pcr.fd
INPUT /usr/share/texlive/texmf-dist/tex/latex/psnfss/t1pcr.fd
INPUT /var/lib/texmf/fonts/pk/ljfour/jknappen/ec/ecrm1000.600pk
INPUT /var/lib/texmf/fonts/map/pdftex/updmap/pdftex.map
INPUT main.aux
OUTPUT main.aux
INPUT sub/../figs/./a.pdf
";

    fn trees() -> Trees {
        Trees {
            texmf_dist: Some("/usr/share/texlive/texmf-dist".into()),
            texmf_var: Some("/home/kevin/.texlive2023/texmf-var".into()),
            texmf_sysvar: Some("/var/lib/texmf".into()),
            texmf_home: Some("/home/kevin/texmf".into()),
            ..Default::default()
        }
    }

    #[test]
    fn parses_and_resolves() {
        let lines = parse_str(SAMPLE, "main.fls").unwrap();
        assert_eq!(lines[0], FlsLine::Pwd("/tmp/build".into()));
        assert_eq!(pwd(&lines), Some(Path::new("/tmp/build")));
        let ins = inputs(&lines);
        assert_eq!(ins.len(), 12, "duplicates collapse: {ins:?}");
        assert_eq!(ins[4], PathBuf::from("/tmp/build/main.tex"));
        assert_eq!(ins[10], PathBuf::from("/tmp/build/main.aux"));
        assert_eq!(ins[11], PathBuf::from("/tmp/build/figs/a.pdf"));
    }

    #[test]
    fn kinds() {
        assert_eq!(InputKind::of(Path::new("/x/ecrm1000.600pk")), InputKind::Pk);
        assert_eq!(InputKind::of(Path::new("/x/a.pk")), InputKind::Other, "a bare .pk is not a bitmap font");
        assert_eq!(InputKind::of(Path::new("/x/t1pcr.fd")), InputKind::Fd);
        assert_eq!(InputKind::of(Path::new("/x/main.out")), InputKind::AuxLike);
        assert_eq!(InputKind::of(Path::new("/x/texmf.cnf")), InputKind::Cnf);
        assert_eq!(InputKind::of(Path::new("/x/README")), InputKind::Other);
        assert_eq!(InputKind::of(Path::new("/x/UnicodeData.txt")), InputKind::Data);
        assert_eq!(InputKind::of(Path::new("/x/beamericonbook.20.pdf")), InputKind::Image);
        assert_eq!(InputKind::of(Path::new("figs/a.PNG")), InputKind::Image);
        assert_eq!(InputKind::of(Path::new("/x/luaotfload-names.luc.gz")), InputKind::Data);
        assert_eq!(
            InputKind::of(Path::new("/x/supp-pdf.mkii")),
            InputKind::Other,
            "an \\input file with an odd extension"
        );
        assert!(InputKind::Tfm.is_font());
        assert!(!InputKind::Map.is_font());
    }

    #[test]
    fn classifies_by_longest_root() {
        let lines = parse_str(SAMPLE, "main.fls").unwrap();
        let files = input_files(&lines, &trees());
        let tree_of = |name: &str| files.iter().find(|f| f.path.ends_with(name)).map(|f| f.tree).unwrap();
        assert_eq!(tree_of("texmf.cnf"), Tree::System, "first texmf.cnf is /etc");
        assert_eq!(tree_of("article.cls"), Tree::TexmfDist);
        assert_eq!(tree_of("pdflatex.fmt"), Tree::TexmfVar);
        assert_eq!(tree_of("ecrm1000.600pk"), Tree::TexmfVar);
        assert_eq!(tree_of("main.tex"), Tree::Project);
        assert_eq!(tree_of("prumo.sty"), Tree::System);
        // A project inside $HOME/texmf-adjacent paths: deeper root wins.
        let mut t = trees();
        t.texmf_home = Some("/home/kevin".into());
        t.project = Some("/home/kevin/docs".into());
        assert_eq!(t.classify(Path::new("/home/kevin/docs/main.tex")), Tree::Project);
        assert_eq!(t.classify(Path::new("/home/kevin/tex/latex/x.sty")), Tree::TexmfHome);
    }

    /// TeX Live on Windows: forward slashes, drive letter in either case, CRLF.
    const WINDOWS_TL: &str = "PWD C:/Users/Kevin/thesis\r
INPUT c:/texlive/2026/texmf-dist/web2c/texmf.cnf\r
INPUT C:/texlive/2026/texmf-var/web2c/pdftex/pdflatex.fmt\r
INPUT ./main.tex\r
INPUT c:/texlive/2026/texmf-dist/tex/latex/base/article.cls\r
INPUT C:/Users/Kevin/thesis/chapters/one.tex\r
INPUT C:/Users/Kevin/texmf/tex/latex/mine/mine.sty\r
OUTPUT main.aux\r
INPUT main.aux\r
";

    #[test]
    fn windows_texlive_paths_resolve_and_classify() {
        let lines = parse_str(WINDOWS_TL, "main.fls").unwrap();
        assert_eq!(pwd(&lines), Some(Path::new("c:/Users/Kevin/thesis")));
        let ins = inputs(&lines);
        assert_eq!(ins[0], PathBuf::from("c:/texlive/2026/texmf-dist/web2c/texmf.cnf"));
        assert_eq!(ins[1], PathBuf::from("c:/texlive/2026/texmf-var/web2c/pdftex/pdflatex.fmt"), "drive lowercased");
        assert_eq!(ins[2], PathBuf::from("c:/Users/Kevin/thesis/main.tex"), "relative to a Windows PWD");
        assert_eq!(outputs(&lines), [PathBuf::from("c:/Users/Kevin/thesis/main.aux")]);
        let trees = Trees::from_kpsewhich(
            "TEXMFDIST=C:/texlive/2026/texmf-dist\nTEXMFSYSVAR=C:/texlive/2026/texmf-var\n\
             TEXMFVAR=C:/Users/Kevin/.texlive2026/texmf-var\nTEXMFHOME=C:/Users/Kevin/texmf\n",
        );
        assert_eq!(trees.texmf_dist.as_deref(), Some(Path::new("c:/texlive/2026/texmf-dist")));
        let files = input_files(&lines, &trees);
        let tree_of = |name: &str| files.iter().find(|f| f.path.ends_with(name)).map(|f| f.tree).unwrap();
        assert_eq!(tree_of("article.cls"), Tree::TexmfDist);
        assert_eq!(tree_of("pdflatex.fmt"), Tree::TexmfVar);
        assert_eq!(tree_of("main.tex"), Tree::Project);
        assert_eq!(tree_of("one.tex"), Tree::Project);
        assert_eq!(tree_of("mine.sty"), Tree::TexmfHome);
        assert_eq!(InputKind::of(&ins[3]), InputKind::Cls);
        assert_eq!(
            crate::tlpdb::Tlpdb::key_for(&ins[3]).as_deref(),
            Some("texmf-dist/tex/latex/base/article.cls"),
            "the tlpdb key does not depend on the drive"
        );
        // Mixed case beyond the drive letter still classifies.
        assert_eq!(trees.classify(Path::new("c:/TeXLive/2026/texmf-dist/tex/x.sty")), Tree::TexmfDist);
        let t = Trees { texmf_dist: Some("C:/TeXLive/2026/texmf-dist".into()), ..Default::default() };
        assert_eq!(t.classify(Path::new("c:/texlive/2026/texmf-dist/tex/x.sty")), Tree::TexmfDist);
        assert_eq!(t.classify(Path::new("c:/texlive/2026/texmf-distro/x.sty")), Tree::System);
    }

    /// MiKTeX writes backslashes, relative paths included.
    #[test]
    fn windows_miktex_backslashes() {
        let text = "PWD C:\\Users\\Kevin\\thesis
INPUT C:\\Users\\Kevin\\AppData\\Local\\Programs\\MiKTeX\\tex\\latex\\base\\article.cls
INPUT .\\main.tex
INPUT chapters\\one.tex
OUTPUT main.aux
";
        let lines = parse_str(text, "main.fls").unwrap();
        let ins = inputs(&lines);
        assert_eq!(ins[0], PathBuf::from("c:/Users/Kevin/AppData/Local/Programs/MiKTeX/tex/latex/base/article.cls"));
        assert_eq!(ins[1], PathBuf::from("c:/Users/Kevin/thesis/main.tex"));
        assert_eq!(ins[2], PathBuf::from("c:/Users/Kevin/thesis/chapters/one.tex"));
        assert_eq!(InputKind::of(&ins[0]), InputKind::Cls);
        let files = input_files(&lines, &Trees::default());
        assert_eq!(files[1].tree, Tree::Project);
        assert_eq!(files[0].tree, Tree::System, "no kpsewhich roots: not texmf-dist");
        // A POSIX file name with a backslash in it is left alone when nothing says Windows.
        let posix = parse_str("PWD /tmp/b\nINPUT /tmp/b/odd\\name.tex\n", "x.fls").unwrap();
        assert_eq!(inputs(&posix)[0], PathBuf::from("/tmp/b/odd\\name.tex"));
    }

    #[test]
    fn cold_luaotfload_cache_is_detected_from_the_outputs() {
        let cold = "PWD /w\nINPUT /w/main.tex\nINPUT /tl/fonts/opentype/public/lm/lmroman10-regular.otf\nINPUT /tl/fonts/truetype/x/DejaVuSans.TTF\nINPUT /tl/fonts/type1/public/cm-super/sfrm1000.pfb\nOUTPUT /home/k/.texlive2026/texmf-var/luatex-cache/generic/names/luaotfload-names.lua.gz\nOUTPUT /home/k/.texlive2026/texmf-var/luatex-cache/generic/names/luaotfload-names.luc.gz\nOUTPUT main.pdf\n";
        let lines = parse_str(cold, "main.fls").unwrap();
        assert_eq!(font_database_built(&lines), Some(3));
        let warm = "PWD /w\nINPUT /w/main.tex\nINPUT /home/k/.texlive2026/texmf-var/luatex-cache/generic/names/luaotfload-names.luc.gz\nOUTPUT main.pdf\n";
        assert_eq!(font_database_built(&parse_str(warm, "main.fls").unwrap()), None, "reading the database is warm");
    }

    #[test]
    fn unknown_line_kind_is_an_error() {
        let e = parse_str("PWD /a\nFOO bar\n", "j.fls").unwrap_err();
        assert_eq!(e.to_string(), "j.fls:2: unknown line kind \"FOO\"");
    }

    #[test]
    fn normalize_keeps_root_and_folds_dots() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("../x")), PathBuf::from("../x"));
    }
}
