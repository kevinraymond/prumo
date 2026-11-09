//! prumo — build analyzer for LaTeX.
//!
//! The library side: parsers for the three artifacts a probed build leaves behind
//! (`records` for `<job>.prumo`, `fls` for `<job>.fls`, `texlog` for the log), the
//! TeX Live package database (`tlpdb`), and the model/report layers on top.
//! `src/main.rs` is the only consumer today. See `docs/03-architecture.md`.

pub mod cache;
pub mod chart;
pub mod compare;
pub mod doctor;
pub mod fls;
pub mod fonts;
pub mod graph;
pub mod html;
pub mod model;
pub mod packages;
pub mod prune;
pub mod records;
pub mod report;
pub mod run;
pub mod texlog;
pub mod tlpdb;
pub mod unused;

/// The `schema` field of every `--json` report. It moves only when a field is renamed or
/// removed, never when one is added.
pub const JSON_SCHEMA: u32 = 1;

/// Scaled seconds: 1/65536 s, the unit of `\sys_timer:` on every engine.
/// Kept integral until display.
pub type Scaled = u64;

/// Scaled seconds per second.
pub const SCALED_PER_SECOND: Scaled = 65_536;

/// Convert a scaled-seconds duration to whole milliseconds, rounding to nearest.
pub fn scaled_to_ms(s: Scaled) -> u64 {
    (s * 1000 + SCALED_PER_SECOND / 2) / SCALED_PER_SECOND
}

/// Convert a scaled-seconds duration to seconds as `f64` (for JSON that mirrors the oracle).
pub fn scaled_to_secs(s: Scaled) -> f64 {
    s as f64 / SCALED_PER_SECOND as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_conversions() {
        assert_eq!(scaled_to_ms(SCALED_PER_SECOND), 1000);
        assert_eq!(scaled_to_ms(655), 10);
        assert_eq!(scaled_to_ms(0), 0);
        assert_eq!(scaled_to_secs(SCALED_PER_SECOND / 2), 0.5);
    }
}
