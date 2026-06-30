//! `x12-core` — a pure-compute, dependency-light parser for ANSI ASC **X12**
//! EDI and **UN/EDIFACT** interchanges.
//!
//! This crate implements only the **public X12 / EDIFACT syntax**: delimiter
//! discovery, segment / element / component / repetition splitting, the
//! ISA/GS/ST (and UNB/UNG/UNH) envelope walk with control-number capture and
//! structural count validation, and **positional** shaped extractors over
//! public segment IDs. It embeds **no** copyrighted ASC X12 TR3
//! implementation-guide text — no loop names, no code-value descriptions; every
//! shaped column is named by its public segment ID and the element's position
//! (`clp_total_paid` = `CLP04`), and raw codes are surfaced verbatim.
//!
//! There is no Arrow or VGI dependency here and no I/O — the worker crate is the
//! thin Arrow adapter. All correctness lives here and is unit-tested directly.
//!
//! # Layout
//!
//! - [`delimiters`] — the four X12 delimiter bytes (+ the EDIFACT release /
//!   decimal bytes) and the fixed-width ISA / UNA sniffers.
//! - [`segment`] — the [`Segment`] row model and the delimiter-driven splitter.
//! - [`envelope`] — the ISA/GS/ST nesting walk, control numbers, and the
//!   `SE`/`GE`/`IEA` structural count + control-match validation.
//! - [`edifact`] — the UNA/UNB/UNG/UNH variant with release-char un-escaping.
//! - [`shaped`] — positional extractors for `835` / `837` / `270` / `271` /
//!   `850` / `997` / `999`.

pub mod delimiters;
pub mod edifact;
pub mod envelope;
pub mod segment;
pub mod shaped;

pub use delimiters::Delimiters;
pub use envelope::{Group, Interchange, Transaction};
pub use segment::Segment;

/// The crate (and worker) version string, surfaced by `x12_version()`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
