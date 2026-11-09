//! A minimal reader for `<job>.log`: the files TeX reported opening (the `(` … `)`
//! nesting), errors, warnings and the output line. prumo prefers the probe's records; the
//! log is a cross-check for files that bypassed the LaTeX file hooks.
//!
//! The approach follows texlogsieve (read before writing this): TeX wraps log lines at
//! `max_print_line` (79 by default), so a line of exactly that length is joined with the
//! next; `(` followed by a path opens a file and `)` closes one; `[`/`]` bracket shipouts;
//! parentheses inside known messages must not count. Unlike texlogsieve, this reader does
//! not try to recognize every message: it only needs the opened-file sequence to be right
//! for the files the records also know about, and it is honest about the rest.

use std::path::{Path, PathBuf};

/// TeX's default `max_print_line`.
pub(crate) const MAX_PRINT_LINE: usize = 79;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LogFile {
    /// Path as printed, e.g. `./main.tex` or `/usr/local/texlive/2026/texmf-dist/tex/latex/base/article.cls`.
    pub path: String,
    /// Nesting depth at the open (0 = opened from the main level).
    pub depth: usize,
    /// 1-based log line of the open (after unwrapping).
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TexLog {
    pub files: Vec<LogFile>,
    /// Lines starting with `! `.
    pub errors: Vec<String>,
    /// `LaTeX Warning:`, `Package foo Warning:`, `Class foo Warning:`, `pdfTeX warning`.
    pub warnings: Vec<String>,
    /// Shipouts counted from `[n` markers.
    pub pages: usize,
    /// `Output written on … (N pages, M bytes).`
    pub output: Option<String>,
    /// Lines that closed more files than were open: the parser lost track at these points.
    pub unbalanced: Vec<usize>,
}

/// Join lines TeX wrapped at `max_print_line`: a line of exactly that length continues on
/// the next one, unless the next line starts a message of its own (texlogsieve's rule:
/// only unwrap when it yields something recognizable). LuaTeX in particular emits lines
/// that are exactly `max_print_line` long and not wrapped at all, and sometimes wraps at
/// `max_print_line + 1`; the latter degrades to two separate lines here.
pub fn unwrap(text: &str, max_print_line: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut joining = false;
    for (i, line) in lines.iter().enumerate() {
        if joining {
            cur.push_str(line);
        } else {
            cur = line.to_string();
        }
        let next = lines.get(i + 1).copied().unwrap_or("");
        joining = line.chars().count() == max_print_line && !starts_message(next);
        if !joining {
            out.push(std::mem::take(&mut cur));
        }
    }
    if joining {
        out.push(cur);
    }
    out
}

/// Lines that begin a message of their own and therefore cannot be a wrapped continuation.
fn starts_message(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("Package: ")
        || line.starts_with("File: ")
        || line.starts_with("Document Class: ")
        || line.starts_with("LaTeX ")
        || line.starts_with("Package ")
        || line.starts_with("Class ")
        || line.starts_with("! ")
        || line.starts_with("\\")
        || line.starts_with("Output written on ")
        || line.starts_with("Overfull ")
        || line.starts_with("Underfull ")
        || line.starts_with("luaotfload")
}

/// Does this token look like a file TeX would print after `(`? A path starts at a root
/// (`/`, `./`, `../`, `C:/`) or its last component has an extension; `m/n` in fontspec's
/// "- 'normal' (m/n) with NFSS spec." is neither.
fn looks_like_path(tok: &str) -> bool {
    let ext_ok = |e: &str| {
        !e.is_empty()
            && e.len() <= 5
            && e.starts_with(|c: char| c.is_ascii_alphabetic())
            && e.chars().all(|c| c.is_ascii_alphanumeric())
    };
    let has_ext = |s: &str| s.rsplit_once('.').is_some_and(|(stem, e)| !stem.is_empty() && ext_ok(e));
    let rooted = tok.starts_with('/')
        || tok.starts_with("./")
        || tok.starts_with("../")
        || tok.starts_with('\\')
        || crate::fls::windows_drive(tok);
    let last = tok.rsplit(['/', '\\']).next().unwrap_or(tok);
    !tok.is_empty() && !tok.contains(',') && (rooted || has_ext(last))
}

/// `(fontspec)             Font family ...`: a continuation line of a package or class
/// message (`\MessageBreak`), never a file open.
fn is_message_continuation(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('(') else { return false };
    let Some((name, after)) = rest.split_once(')') else { return false };
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.contains('.')
        && after.starts_with("  ")
}

/// Parse log text (already read from disk; `lossy` conversion is fine).
pub(crate) fn parse_str(text: &str) -> TexLog {
    let mut log = TexLog::default();
    let mut depth = 0usize;
    for (idx, line) in unwrap(text, MAX_PRINT_LINE).into_iter().enumerate() {
        let n = idx + 1;
        if let Some(e) = line.strip_prefix("! ") {
            log.errors.push(e.to_string());
            continue;
        }
        if line.starts_with("LaTeX Warning:")
            || line.starts_with("LaTeX Font Warning:")
            || (line.starts_with("Package ") && line.contains(" Warning:"))
            || (line.starts_with("Class ") && line.contains(" Warning:"))
            || line.starts_with("pdfTeX warning")
        {
            log.warnings.push(line.clone());
            continue;
        }
        if line.starts_with("Output written on ") {
            log.output = Some(line.trim_end_matches('.').to_string());
            continue;
        }
        // Messages that carry parentheses but are not file opens.
        if line.starts_with("LaTeX2e <")
            || line.starts_with("L3 programming layer")
            || line.starts_with("Package: ")
            || line.starts_with("Document Class: ")
            || line.starts_with("File: ")
            || line.starts_with("\\openout")
            || is_message_continuation(&line)
        {
            continue;
        }
        let bytes: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                '(' => {
                    let start = i + 1;
                    let mut j = start;
                    while j < bytes.len()
                        && !bytes[j].is_whitespace()
                        && bytes[j] != '('
                        && bytes[j] != ')'
                        && bytes[j] != '['
                        && bytes[j] != ']'
                    {
                        j += 1;
                    }
                    let tok: String = bytes[start..j].iter().collect();
                    if looks_like_path(&tok) {
                        log.files.push(LogFile { path: tok, depth, line: n });
                        depth += 1;
                        i = j;
                        continue;
                    }
                    // A parenthesized remark, not a file: skip to its close if on this line.
                    i += 1;
                    let mut inner = 1;
                    while i < bytes.len() && inner > 0 {
                        match bytes[i] {
                            '(' => inner += 1,
                            ')' => inner -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    continue;
                }
                ')' => {
                    if depth == 0 {
                        log.unbalanced.push(n);
                    } else {
                        depth -= 1;
                    }
                }
                // `[1`, `[2{/path/pdftex.map}]`: a shipout starts with a digit.
                '[' if bytes.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => log.pages += 1,
                _ => {}
            }
            i += 1;
        }
    }
    log
}

#[derive(Debug, thiserror::Error)]
pub enum TexLogError {
    #[error("cannot read {file}: {source}")]
    Io {
        file: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn parse_file(path: &Path) -> Result<TexLog, TexLogError> {
    let bytes = std::fs::read(path).map_err(|source| TexLogError::Io { file: path.to_path_buf(), source })?;
    Ok(parse_str(&String::from_utf8_lossy(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fontspec_messages_are_not_file_opens() {
        let log = parse_str(
            "(./glyphs.tex (/tl/fontspec.sty\n(fontspec)             Font family 'lmroman10-regular.otf(0)' created for font\n(fontspec)             - 'normal' (m/n) with NFSS spec.:\n(Font)                  OT1/cmr/m/n --> TU/lmroman10-regular.otf(0)/m/n on input line 14.\n) [1] )\n",
        );
        let files: Vec<&str> = log.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["./glyphs.tex", "/tl/fontspec.sty"]);
        assert!(log.unbalanced.is_empty(), "{:?}", log.unbalanced);
        assert_eq!(log.pages, 1);
        assert!(looks_like_path("./main.tex") && looks_like_path("/a/b.sty") && looks_like_path("size10.clo"));
        assert!(looks_like_path("c:/texlive/2026/texmf-dist/tex/latex/base/article.cls"));
        assert!(!looks_like_path("m/n") && !looks_like_path("Font") && !looks_like_path("0"));
        assert!(is_message_continuation("(fontspec)             - 'bold' (b/n) with NFSS spec.:"));
        assert!(!is_message_continuation("(./main.tex"));
        assert!(!is_message_continuation("(/tl/x.sty)"));
    }

    #[test]
    fn unwraps_lines_of_exactly_max_print_line() {
        let a = "x".repeat(MAX_PRINT_LINE);
        let text = format!("{a}\ncontinued\nshort\n");
        let lines = unwrap(&text, MAX_PRINT_LINE);
        assert_eq!(lines, vec![format!("{a}continued"), "short".to_string()]);
        let lines = unwrap("a\nb", MAX_PRINT_LINE);
        assert_eq!(lines, vec!["a", "b"]);
        // A line of exactly max_print_line followed by a package banner is not wrapped.
        let path = format!("({}x.sty", "/".repeat(MAX_PRINT_LINE - 6));
        assert_eq!(path.len(), MAX_PRINT_LINE);
        let lines = unwrap(&format!("{path}\nPackage: x 2026/01/01\n"), MAX_PRINT_LINE);
        assert_eq!(lines, vec![path, "Package: x 2026/01/01".to_string()]);
    }

    #[test]
    fn files_errors_warnings_and_pages() {
        let text = "This is pdfTeX, Version 3.141592653 (TeX Live 2026)\n\
LaTeX2e <2026-11-01>\n\
(./main.tex (/usr/local/texlive/2026/texmf-dist/tex/latex/base/article.cls\n\
Document Class: article 2024/06/29 v1.4n Standard LaTeX document class\n\
(/usr/local/texlive/2026/texmf-dist/tex/latex/base/size10.clo)) (./main.aux)\n\
LaTeX Warning: Reference `x' on page 1 undefined on input line 3.\n\
Package hyperref Warning: Something.\n\
[1{/usr/local/texlive/2026/texmf-var/fonts/map/pdftex/updmap/pdftex.map}] [2]\n\
! Undefined control sequence.\n\
(see the transcript file for additional information)\n\
Output written on main.pdf (2 pages, 65450 bytes).\n";
        let log = parse_str(text);
        let paths: Vec<_> = log.files.iter().map(|f| (f.path.as_str(), f.depth)).collect();
        assert_eq!(
            paths,
            [
                ("./main.tex", 0),
                ("/usr/local/texlive/2026/texmf-dist/tex/latex/base/article.cls", 1),
                ("/usr/local/texlive/2026/texmf-dist/tex/latex/base/size10.clo", 2),
                ("./main.aux", 1)
            ]
        );
        assert_eq!(log.errors, ["Undefined control sequence."]);
        assert_eq!(log.warnings.len(), 2);
        assert_eq!(log.pages, 2);
        assert_eq!(log.output.as_deref(), Some("Output written on main.pdf (2 pages, 65450 bytes)"));
        assert!(log.unbalanced.is_empty(), "{:?}", log.unbalanced);
    }

    #[test]
    fn remarks_in_parentheses_are_not_files() {
        let log = parse_str(
            "(./a.tex (This is a remark) (./b.sty) 12 objects out of 1000 (max. 8388607) (0.0pt,0.0pt) (72.27pt))\n",
        );
        let paths: Vec<_> = log.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["./a.tex", "./b.sty"]);
        assert!(log.unbalanced.is_empty());
    }
}
