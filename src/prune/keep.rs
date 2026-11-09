//! The keep set: packages the `.fls` cannot prove but every TeX Live needs, each with its
//! reason. See `docs/05-packages-and-pruning.md`, "The keep set".

use crate::tlpdb::Tlpdb;
use std::collections::BTreeMap;

/// Why a package stays installed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum KeepReason {
    /// A document read one of its files; `via` is the LaTeX unit that loaded it.
    Document { doc: String, file: String, via: String },
    /// TeX Live's own machinery: tlmgr, kpathsea, the scripts.
    Infra { note: &'static str },
    /// An engine a document ran on.
    Engine { doc: String },
    /// Provides a format a document ran, or is one of its `fmttriggers`.
    Format { format: String },
    /// Hyphenation patterns (`--keep-hyphenation all`, the default, or `used`).
    Hyphenation { note: String },
    /// `--keep NAME`.
    User,
    /// XeTeX/LuaTeX: OpenType/TrueType fonts the PDF names come from system-level loading
    /// the recorder does not see; the provider is kept by file-name match, or every provider
    /// is kept when a name cannot be matched.
    OpenTypeProvider { font: String },
    /// `latexmk` drove a build.
    Latexmk,
    /// A build tool the `.fls` proves ran or is needed: biber (a `.bcf` was written),
    /// bibtex (a `.bbl` was read and no `.bcf` written), makeindex (an `.ind` was read).
    Tool { doc: String },
    /// Pulled in by another kept package's `depend`.
    Depend { of: String },
}

/// Packages always kept, with the reason. Order is the order they appear in the report.
pub fn infra() -> Vec<(&'static str, KeepReason)> {
    vec![
        ("texlive.infra", KeepReason::Infra { note: "tlmgr itself" }),
        ("kpathsea", KeepReason::Infra { note: "path searching, texmf.cnf" }),
        ("texlive-scripts", KeepReason::Infra { note: "fmtutil, updmap, mktexlsr" }),
        ("hyphen-base", KeepReason::Infra { note: "hyphenation infrastructure, formats need it" }),
    ]
}

/// Every installed `hyphen-*` package (`--keep-hyphenation all`), or those whose
/// `AddHyphen name=`/`synonyms=` names a language the corpus loaded (`used`, matched on
/// `<lang>.ldf` basenames; best effort).
pub fn hyphenation(db: &Tlpdb, used_languages: &[String], all: bool) -> BTreeMap<String, KeepReason> {
    let mut out = BTreeMap::new();
    for (name, p) in &db.packages {
        if !name.starts_with("hyphen-") || name == "hyphen-base" {
            continue;
        }
        if all {
            out.insert(name.clone(), KeepReason::Hyphenation { note: "--keep-hyphenation all".into() });
            continue;
        }
        for e in p.executes.iter().filter(|e| e.verb == "AddHyphen") {
            let names: Vec<&str> = e
                .args
                .split_whitespace()
                .filter_map(|a| a.strip_prefix("name=").or_else(|| a.strip_prefix("synonyms=")))
                .flat_map(|v| v.split(','))
                .collect();
            if let Some(lang) = used_languages.iter().find(|l| names.iter().any(|n| n.eq_ignore_ascii_case(l))) {
                out.insert(name.clone(), KeepReason::Hyphenation { note: format!("{lang}.ldf loaded") });
                break;
            }
        }
    }
    out
}

/// Packages that ship an OpenType/TrueType file whose stem matches a PDF font name
/// (`LMRoman10-Regular` → `lmroman10-regular.otf`). Empty when nothing matches.
pub fn opentype_providers(db: &Tlpdb, font_name: &str) -> Vec<String> {
    // XeTeX names CID-keyed fonts `<name>-Identity-H`; the file is named without the CMap.
    let base =
        font_name.strip_suffix("-Identity-H").or_else(|| font_name.strip_suffix("-Identity-V")).unwrap_or(font_name);
    let want = base.to_ascii_lowercase();
    let mut v: Vec<String> = db
        .packages
        .values()
        .filter(|p| {
            p.runfiles.iter().any(|f| {
                (f.contains("/fonts/opentype/") || f.contains("/fonts/truetype/"))
                    && f.rsplit('/')
                        .next()
                        .and_then(|b| b.rsplit_once('.'))
                        .is_some_and(|(stem, _)| stem.eq_ignore_ascii_case(&want))
            })
        })
        .map(|p| p.name.clone())
        .collect();
    v.sort();
    v
}

/// Every package shipping OpenType/TrueType fonts: the fallback when a name has no match.
pub fn all_opentype_providers(db: &Tlpdb) -> Vec<String> {
    let mut v: Vec<String> = db
        .packages
        .values()
        .filter(|p| p.runfiles.iter().any(|f| f.contains("/fonts/opentype/") || f.contains("/fonts/truetype/")))
        .map(|p| p.name.clone())
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn opentype_match_strips_the_cmap_suffix() {
        let db = crate::tlpdb::parse_file(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tlpdb/excerpt.tlpdb"))
            .unwrap();
        assert_eq!(opentype_providers(&db, "LMRoman10-Regular"), ["lm"]);
        assert_eq!(opentype_providers(&db, "LMRoman10-Regular-Identity-H"), ["lm"]);
        assert!(opentype_providers(&db, "NoSuchFont-Bold").is_empty());
        assert!(all_opentype_providers(&db).contains(&"lm".to_string()));
        let h = hyphenation(&db, &["portuguese".into()], false);
        assert!(h.contains_key("hyphen-portuguese"), "{h:?}");
        assert!(!h.contains_key("hyphen-english"));
        assert_eq!(hyphenation(&db, &[], true).len(), 2, "both hyphen-* stanzas in the excerpt");
    }
}
