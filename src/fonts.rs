//! `prumo fonts`: fonts loaded (TFMs read) vs fonts embedded (in the PDF), resolved through
//! the font map, with Type 3 / bitmap warnings and attribution through the `.fd` frame.
//!
//! Mirrors `font_report()` in `scripts/prumo_report.py` field for field, then adds what the
//! oracle does not do: which LaTeX unit loaded each font family.
//!
//! Sources, by engine (`docs/04-probe.md`, findings 9–14):
//! - loaded: `.tfm` INPUT lines of the `.fls`. Fonts the format preloads (`cmr10`…) never
//!   appear there; they are named from the PDF through the reverse map (`preloaded`).
//! - embedded, pdfTeX: the `.pfb`/`.pk` INPUT lines are exact. All engines: `pdffonts`.
//! - names: `pdftex.map` lines `<tfm> <psname> … <<file.pfb` tie TFM names to PostScript
//!   names and outline files. XeTeX's `.fls` lacks the map (xdvipdfmx reads it); `run.rs`
//!   finds it with `kpsewhich` and tests use the saved `fontmap.txt`.

use crate::doctor::Installer;
use crate::fls::{InputFile, InputKind};
use crate::model::BuildModel;
use crate::records::Engine;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One `pdftex.map` line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MapEntry {
    pub tfm: String,
    pub ps_name: Option<String>,
    /// `sfrm1000.pfb`, `lmr10.pfb`, a `.ttf`/`.otf`; `None` for bitmap-only entries.
    pub file: Option<String>,
    pub encoding: Option<String>,
}

/// Parse map text into a `tfm → entry` index; the first line for a TFM wins (as pdfTeX does).
pub fn parse_map(text: &str) -> HashMap<String, MapEntry> {
    parse_map_entries(text).into_iter().map(|e| (e.tfm.clone(), e)).collect()
}

/// Parse map text in file order, keeping the first line per TFM.
pub fn parse_map_entries(text: &str) -> Vec<MapEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('%') || line.starts_with('#') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        let tfm = toks[0].to_string();
        let ps_name = toks.get(1).filter(|t| !t.starts_with('<') && !t.starts_with('"')).map(|t| t.to_string());
        let files: Vec<&str> =
            toks.iter().filter(|t| t.starts_with('<')).map(|t| t.trim_start_matches(['<', '['])).collect();
        let is_outline = |f: &str| {
            let l = f.to_ascii_lowercase();
            l.ends_with(".pfb") || l.ends_with(".pfa") || l.ends_with(".ttf") || l.ends_with(".otf")
        };
        let file = files.iter().find(|f| is_outline(f)).map(|f| f.to_string());
        let encoding = files.iter().find(|f| f.to_ascii_lowercase().ends_with(".enc")).map(|f| f.to_string());
        if seen.insert(tfm.clone()) {
            out.push(MapEntry { tfm, ps_name, file, encoding });
        }
    }
    out
}

/// One line of `pdffonts` output.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PdfFont {
    /// `CMR10` for `AUUZAN+CMR10`; `F44` for an anonymous Type 3 font.
    pub name: String,
    /// `Type 1`, `Type 1C`, `Type 3`, `TrueType`, `CID Type 0C`, …
    pub kind: String,
    /// The oracle's sloppy "type" column: the second and third whitespace tokens joined.
    pub type_raw: String,
    pub encoding: String,
    pub embedded: bool,
    pub subset: bool,
}

/// Parse `pdffonts` text: two header lines, then
/// `name type encoding emb sub uni object ID` with a multi-word type.
pub fn parse_pdffonts(text: &str) -> Vec<PdfFont> {
    let mut out = Vec::new();
    for line in text.lines().skip(2) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 8 {
            continue;
        }
        // From the right: `<obj> <gen>`, uni, sub, emb, encoding; what is left after the name is the type.
        let n = cols.len();
        let (emb, sub, encoding) = (cols[n - 5], cols[n - 4], cols[n - 6]);
        let kind = cols[1..n - 6].join(" ");
        out.push(PdfFont {
            name: cols[0].rsplit('+').next().unwrap_or(cols[0]).to_string(),
            kind,
            type_raw: cols[1..3.min(n)].join(" "),
            encoding: encoding.to_string(),
            embedded: emb == "yes",
            subset: sub == "yes",
        });
    }
    out
}

/// A TFM's fate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Its outline file was read, or its PostScript name is in the PDF.
    Embedded,
    /// Loaded, but no glyph was ever set: nothing embedded.
    NotEmbedded,
    /// A virtual font (`.vf` read); its raw fonts are listed on their own.
    Virtual,
    /// Not in the map: a bitmap (`.pk`) font, a virtual font never used, or unknown.
    NoMapEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OpenTypeFont {
    /// The PostScript name without XeTeX's `-Identity-H` suffix.
    pub name: String,
    /// The `.otf`/`.ttf` the build read, or the `.luc` cache luaotfload read instead, when
    /// the `.fls` has one with that stem.
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LoadedFont {
    pub tfm: String,
    pub ps_name: Option<String>,
    pub status: Status,
    /// `T1/pcr via t1pcr.fd, loaded by main.tex` when a read `.fd` declares the TFM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct FontReport {
    pub schema: u32,
    pub job: String,
    pub engine: String,
    pub loaded: Vec<LoadedFont>,
    pub embedded_in_pdf: Vec<PdfFont>,
    pub type3: usize,
    pub bitmap_pk: Vec<String>,
    /// Embedded fonts the map resolves to a TFM that was never read: preloaded by the format
    /// (cmr10 and friends).
    pub preloaded: Vec<String>,
    /// OpenType fonts in the PDF (CID or TrueType): fontspec on XeTeX/LuaTeX. XeTeX opens
    /// them itself, so the `.fls` lists none and `file` is `None`; LuaTeX reads them through
    /// kpathsea and the file is paired by its stem (the `.otf`, or the `.luc` luaotfload
    /// wrote on an earlier run).
    pub opentype: Vec<OpenTypeFont>,
    /// Embedded fonts neither the map nor an OpenType file accounts for: usually fonts inside
    /// included graphics.
    pub unknown_embedded: Vec<String>,
    /// Type 3 fonts are anonymous in `pdffonts`; the match is partial when any are present.
    pub partial: bool,
    pub map_entries: usize,
    pub warnings: Vec<String>,
    // --- the oracle's naive fields, kept so the golden comparison stays exact ---
    pub tfm_loaded: Vec<String>,
    pub loaded_not_embedded_naive: Vec<String>,
}

/// `tfm → (encoding/family, fd file)` from the `.fd` files the run read.
/// `\DeclareFontShape{T1}{pcr}{m}{n}{ <-> pcrr8t }{}` declares `pcrr8t` for `T1/pcr`.
pub fn fd_declarations(fd_texts: &[(String, String)]) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    for (fd_name, text) in fd_texts {
        let text: String = text.lines().map(|l| l.split('%').next().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let mut rest = text.as_str();
        while let Some(i) = rest.find("\\DeclareFontShape") {
            rest = &rest[i + "\\DeclareFontShape".len()..];
            let mut args = Vec::new();
            let mut r = rest;
            for _ in 0..5 {
                let Some((arg, after)) = brace_arg(r) else { break };
                args.push(arg);
                r = after;
            }
            if args.len() == 5 {
                let family = format!("{}/{}", args[0].trim(), args[1].trim());
                for tfm in shape_fonts(args[4]) {
                    out.entry(tfm).or_insert((family.clone(), fd_name.clone()));
                }
            }
        }
    }
    out
}

/// The external font names in a `\DeclareFontShape` size/shape spec:
/// `<-> pcrr8t`, `<5><6><7>genb*cmr<10.95>cmr10<14.4>cmr12`, `<->s*[0.95]lmr10`,
/// `<->ssub*cmr/bx/n` (a substitution, no font of its own).
fn shape_fonts(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut sizes: Vec<String> = Vec::new();
    let mut rest = spec.trim();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('<') {
            let end = r.find('>').unwrap_or(r.len());
            sizes.push(r[..end].trim().to_string());
            rest = r.get(end + 1..).unwrap_or("").trim_start();
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        let info = rest[..end].trim();
        rest = rest[end..].trim_start();
        let (keyword, arg) = match info.split_once('*') {
            Some((k, a)) => (k.trim(), a.trim()),
            None => ("", info),
        };
        // `[0.95] name`: a scaling prefix.
        let arg = arg.strip_prefix('[').and_then(|a| a.split_once(']')).map(|(_, n)| n.trim()).unwrap_or(arg);
        match keyword {
            "ssub" | "sub" => {}
            "gen" | "genb" | "sgen" | "sgenb" => {
                for size in &sizes {
                    if !size.contains('-') && !size.contains('.') && !size.is_empty() {
                        out.push(format!("{arg}{size}"));
                    }
                }
            }
            _ if !arg.is_empty() && !arg.contains('/') => out.push(arg.to_string()),
            _ => {}
        }
        sizes.clear();
    }
    out
}

/// `{...}` with nesting, skipping leading whitespace. Returns (inside, after).
fn brace_arg(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let s = s.strip_prefix('{')?;
    let mut depth = 1;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn stem(f: &InputFile) -> String {
    f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn base(f: &InputFile) -> String {
    f.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Build the report. `map_text` is `pdftex.map` (or the saved subset); `pdffonts_text` is
/// `pdffonts` output; `fd_texts` are `(file name, contents)` of the `.fd` files read;
/// `installer` picks the wording of the install hint (`tlmgr` or MiKTeX).
pub fn report(
    job: &str,
    model: &BuildModel,
    files: &[InputFile],
    map_text: Option<&str>,
    pdffonts_text: Option<&str>,
    fd_texts: &[(String, String)],
    installer: Installer,
) -> FontReport {
    let loaded: BTreeSet<String> = files.iter().filter(|f| f.kind == InputKind::Tfm).map(stem).collect();
    let virtual_fonts: BTreeSet<String> = files.iter().filter(|f| f.kind == InputKind::Vf).map(stem).collect();
    let outlines: BTreeSet<String> = files
        .iter()
        .filter(|f| matches!(f.kind, InputKind::Pfb | InputKind::Pfa | InputKind::Otf | InputKind::Ttf))
        .map(base)
        .collect();
    let bitmap_pk: Vec<String> =
        files.iter().filter(|f| f.kind == InputKind::Pk).map(base).collect::<BTreeSet<_>>().into_iter().collect();
    let entries = map_text.map(parse_map_entries).unwrap_or_default();
    let map: HashMap<&str, &MapEntry> = entries.iter().map(|e| (e.tfm.as_str(), e)).collect();
    let pdf = pdffonts_text.map(parse_pdffonts).unwrap_or_default();
    let type3 = pdf.iter().filter(|f| f.kind == "Type 3").count();
    let emb_lower: BTreeSet<String> = pdf.iter().map(|f| f.name.to_lowercase()).collect();
    let decls = fd_declarations(fd_texts);
    let loaders = crate::packages::loaders(model);

    let mut rows = Vec::new();
    for tfm in &loaded {
        let entry = map.get(tfm.as_str()).copied();
        let ps_name = entry.and_then(|e| e.ps_name.clone());
        let status = if virtual_fonts.contains(tfm) {
            Status::Virtual
        } else if let Some(e) = entry {
            let file_read = e.file.as_ref().is_some_and(|f| outlines.contains(f));
            let in_pdf = e.ps_name.as_ref().is_some_and(|p| emb_lower.contains(&p.to_lowercase()))
                || emb_lower.contains(&tfm.to_lowercase());
            if file_read || in_pdf { Status::Embedded } else { Status::NotEmbedded }
        } else {
            Status::NoMapEntry
        };
        let declared_by = decls.get(tfm).map(|(family, fd)| {
            let by = loaders.get(fd).map(|l| format!(", loaded by {l}")).unwrap_or_default();
            format!("{family} via {fd}{by}")
        });
        rows.push(LoadedFont { tfm: tfm.clone(), ps_name, status, declared_by });
    }

    // Preloaded: embedded names no loaded TFM accounts for.
    let loaded_ps: BTreeSet<String> =
        rows.iter().filter_map(|r| r.ps_name.as_ref().map(|p| p.to_lowercase())).collect();
    let loaded_lower: BTreeSet<String> = loaded.iter().map(|t| t.to_lowercase()).collect();
    // PostScript name → first TFM in map order, as the oracle does.
    let mut reverse: BTreeMap<String, String> = BTreeMap::new();
    for e in &entries {
        if let Some(p) = &e.ps_name {
            reverse.entry(p.to_lowercase()).or_insert_with(|| e.tfm.clone());
        }
    }
    // Embedded names no loaded TFM accounts for, in three kinds (as the oracle does):
    // OpenType (CID or TrueType), preloaded (the map resolves the name to a TFM never read),
    // unknown (neither: fonts inside included graphics).
    // LuaTeX with a warm luaotfload cache reads only the cached tables (<stem>.luc), not the .otf.
    let cached_fonts = files.iter().filter(|f| f.path.extension().is_some_and(|e| e == "luc")).map(base);
    let stems: BTreeMap<String, String> = cached_fonts
        .chain(outlines.iter().cloned())
        .map(|f| (f.rsplit_once('.').map(|(s, _)| s).unwrap_or(&f).to_lowercase(), f))
        .collect();
    let mut opentype: BTreeMap<String, OpenTypeFont> = BTreeMap::new();
    let mut preloaded = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    for f in &pdf {
        let lower = f.name.to_lowercase();
        if f.kind == "Type 3" || loaded_ps.contains(&lower) || loaded_lower.contains(&lower) {
            continue;
        }
        if f.kind.contains("CID") || f.kind.contains("TrueType") {
            let name = f.name.strip_suffix("-Identity-H").or(f.name.strip_suffix("-Identity-V")).unwrap_or(&f.name);
            opentype.entry(name.to_string()).or_insert_with(|| OpenTypeFont {
                name: name.to_string(),
                file: stems.get(&name.to_lowercase()).cloned(),
            });
        } else if let Some(t) = reverse.get(&lower) {
            preloaded.insert(t.clone());
        } else {
            unknown.insert(f.name.clone());
        }
    }
    let preloaded: Vec<String> = preloaded.into_iter().collect();
    let opentype: Vec<OpenTypeFont> = opentype.into_values().collect();
    let unknown_embedded: Vec<String> = unknown.into_iter().collect();

    let mut warnings = Vec::new();
    if type3 > 0 || !bitmap_pk.is_empty() {
        warnings.push(format!(
            "{} Type 3 (bitmap) font{} in the PDF{}: usually a missing cm-super or lmodern. Fix: {}, or `\\usepackage{{lmodern}}`.",
            type3,
            if type3 == 1 { "" } else { "s" },
            if bitmap_pk.is_empty() { String::new() } else { format!(" ({} .pk files generated by mktexpk)", bitmap_pk.len()) },
            installer.install("cm-super"),
        ));
    }
    if map.is_empty() {
        warnings.push("no font map available: TFM and PostScript names could not be tied together".into());
    }
    let unpaired: Vec<&str> = opentype.iter().filter(|o| o.file.is_none()).map(|o| o.name.as_str()).collect();
    if !unpaired.is_empty() {
        warnings.push(if model.engine == Engine::XeTex {
            "XeTeX opens OpenType fonts itself, outside kpathsea: the .fls lists none of them, so they cannot be paired with files or packages from this run; the PDF side (pdffonts) is complete".to_string()
        } else {
            format!("OpenType fonts in the PDF with no matching file in the .fls: {}", unpaired.join(", "))
        });
    }
    let naive: Vec<String> = loaded.iter().filter(|t| !emb_lower.contains(&t.to_lowercase())).cloned().collect();
    FontReport {
        schema: 1,
        job: job.to_string(),
        engine: model.engine.to_string(),
        loaded: rows,
        embedded_in_pdf: pdf,
        type3,
        bitmap_pk,
        preloaded,
        opentype,
        unknown_embedded,
        partial: type3 > 0,
        map_entries: map.len(),
        warnings,
        tfm_loaded: loaded.into_iter().collect(),
        loaded_not_embedded_naive: naive,
    }
}

pub fn text(r: &FontReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "== prumo fonts: {} ({})  TFMs loaded: {}, embedded in PDF: {}, Type 3: {}, bitmap .pk read: {}",
        r.job,
        r.engine,
        r.loaded.len(),
        r.embedded_in_pdf.len(),
        r.type3,
        r.bitmap_pk.len()
    );
    for w in &r.warnings {
        let _ = writeln!(s, "  WARNING: {w}");
    }
    let w = r.loaded.iter().map(|f| f.tfm.len()).max().unwrap_or(8);
    let _ = writeln!(s, "\n-- loaded --");
    for f in &r.loaded {
        let status = match f.status {
            Status::Embedded => "embedded",
            Status::NotEmbedded => "loaded, never used",
            Status::Virtual => "virtual font",
            Status::NoMapEntry => "no map entry",
        };
        let ps = f.ps_name.as_deref().map(|p| format!(" = {p}")).unwrap_or_default();
        let by = f.declared_by.as_deref().map(|d| format!("  [{d}]")).unwrap_or_default();
        let _ = writeln!(s, "  {:<w$}{ps:<24} {status}{by}", f.tfm);
    }
    if !r.preloaded.is_empty() {
        let _ = writeln!(s, "\n-- preloaded by the format, embedded --\n  {}", r.preloaded.join(", "));
    }
    if !r.opentype.is_empty() {
        let _ = writeln!(s, "\n-- OpenType, opened by the engine --");
        for o in &r.opentype {
            match &o.file {
                Some(f) => {
                    let _ = writeln!(s, "  {:<28} {f}", o.name);
                }
                None => {
                    let _ = writeln!(s, "  {:<28} (not in the .fls)", o.name);
                }
            }
        }
    }
    if !r.unknown_embedded.is_empty() {
        let _ =
            writeln!(s, "\n-- in the PDF, from included graphics or unknown --\n  {}", r.unknown_embedded.join(", "));
    }
    let unused: Vec<&str> =
        r.loaded.iter().filter(|f| f.status == Status::NotEmbedded).map(|f| f.tfm.as_str()).collect();
    if !unused.is_empty() {
        let _ = writeln!(
            s,
            "\n  hint: {} font{} loaded but never set a glyph ({}); each costs a TFM read, nothing in the PDF.",
            unused.len(),
            if unused.len() == 1 { "" } else { "s" },
            unused.join(", ")
        );
    }
    if r.partial {
        let _ = writeln!(s, "  note: Type 3 fonts are anonymous in the PDF, so the loaded/embedded match is partial.");
    }
    let _ = writeln!(s, "\n-- in the PDF --");
    for f in &r.embedded_in_pdf {
        let _ = writeln!(
            s,
            "  {:<28} {:<12} {}{}",
            f.name,
            f.kind,
            if f.embedded { "embedded" } else { "NOT embedded" },
            if f.subset { ", subset" } else { "" }
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_lines() {
        let m = parse_map(
            "% comment\necrm1000 SFRM1000 \" T1Encoding ReEncodeFont \" <cm-super-t1.enc <sfrm1000.pfb\ncmr10 CMR10 <cmr10.pfb\nec-lmr10 LMRoman10-Regular \" enclmec ReEncodeFont \" <lm-ec.enc <<lmr10.pfb\ncmr10 DUPLICATE <x.pfb\nbitmap-only <[weird.enc\n",
        );
        assert_eq!(m["ecrm1000"].ps_name.as_deref(), Some("SFRM1000"));
        assert_eq!(m["ecrm1000"].file.as_deref(), Some("sfrm1000.pfb"));
        assert_eq!(m["ecrm1000"].encoding.as_deref(), Some("cm-super-t1.enc"));
        assert_eq!(m["cmr10"].ps_name.as_deref(), Some("CMR10"), "first line wins");
        assert_eq!(m["ec-lmr10"].file.as_deref(), Some("lmr10.pfb"), "<< prefix stripped");
        assert_eq!(m["bitmap-only"].ps_name, None);
        assert_eq!(m["bitmap-only"].file, None);
    }

    #[test]
    fn pdffonts_lines() {
        let p = parse_pdffonts(
            "name                                 type              encoding         emb sub uni object ID\n------------------------------------ ----------------- ---------------- --- --- --- ---------\nF44                                  Type 3            Custom           yes no  no      10  0\nAUUZAN+CMMI10                        Type 1            Builtin          yes yes yes     12  0\nQVIUWV+LMRoman12-Bold                Type 1C           WinAnsi          yes yes yes      4  0\nABCDEF+NotoSansCJK-Regular           CID Type 0C       Identity-H       yes yes yes     20  0\n",
        );
        assert_eq!(p.len(), 4);
        assert_eq!((p[0].name.as_str(), p[0].kind.as_str(), p[0].subset), ("F44", "Type 3", false));
        assert_eq!((p[1].name.as_str(), p[1].kind.as_str(), p[1].type_raw.as_str()), ("CMMI10", "Type 1", "Type 1"));
        assert_eq!(
            (p[2].name.as_str(), p[2].kind.as_str(), p[2].type_raw.as_str()),
            ("LMRoman12-Bold", "Type 1C", "Type 1C")
        );
        assert_eq!(
            (p[3].name.as_str(), p[3].kind.as_str(), p[3].encoding.as_str()),
            ("NotoSansCJK-Regular", "CID Type 0C", "Identity-H")
        );
        assert!(p[3].embedded);
    }

    #[test]
    fn fd_declarations_find_tfms() {
        let fd = "\\DeclareFontFamily{T1}{pcr}{\\hyphenchar \\font\\m@ne}\n\\DeclareFontShape{T1}{pcr}{m}{n}{\n   <-> pcrr8t\n}{}\n\\DeclareFontShape{T1}{pcr}{bx}{n}{<->ssub * pcr/b/n}{}\n\\DeclareFontShape{OT1}{cmr}{m}{n}{<5><6><7><8><9><10><12>genb*cmr<10.95>cmr10<14.4>cmr12}{} % comment\n";
        let d = fd_declarations(&[("t1pcr.fd".into(), fd.into())]);
        assert_eq!(d["pcrr8t"], ("T1/pcr".to_string(), "t1pcr.fd".to_string()));
        assert!(!d.contains_key("pcr/b/n"), "substitutions are not TFMs: {d:?}");
        assert_eq!(d.get("cmr10").map(|x| x.0.as_str()), Some("OT1/cmr"));
        assert_eq!(d.get("cmr5").map(|x| x.0.as_str()), Some("OT1/cmr"), "genb*cmr expands over the listed sizes");
        assert!(d.contains_key("cmr12") && !d.contains_key("cmr10.95"));
        assert_eq!(shape_fonts("<->s*[0.95]lmr10"), ["lmr10"]);
        assert_eq!(shape_fonts("<-> fixed * cmr10"), ["cmr10"]);
    }
}
