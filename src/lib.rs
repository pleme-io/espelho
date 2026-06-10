//! espelho (mirror) — the typed terminal-conformance contract.
//!
//! One shape, consumed from both sides of the PTY:
//!
//! - a **host** is anything that owns the terminal side and answers VT
//!   queries (mado's `Terminal`, tear's `PaneGrid`, a test
//!   [`TerminalPersona`]);
//! - a **guest** is anything that runs *inside* a terminal and must stay
//!   live regardless of how the host (mis)behaves (frost, the frostmourne
//!   distribution, escriba's TUI).
//!
//! "Together and apart": each side conforms alone against this contract,
//! and every host × guest composition is then a mechanical matrix row —
//! the same invariants, no bespoke glue per pair.
//!
//! TYPED-SPEC TRIPLET (per the org rule):
//! 1. this typed Rust border (`VtQuery`, `VtAnswer`, `AnswerPolicy`,
//!    `TerminalPersona`, `ConformanceOutcome`),
//! 2. the authored Lisp spec at `specs/term-conformance.lisp`
//!    (`(defterm-conformance …)` — the query catalog, canonical personas,
//!    and invariants as data),
//! 3. the working interpreter [`apply`] over the mockable [`TermEnv`]
//!    environment trait (tests mock it; the L0/L1 harnesses implement it
//!    over a real PTY).
//!
//! Incident lineage (2026-06-10, the reason this crate exists): a lost
//! CPR answer fatally killed the fleet shell; an unanswering terminal
//! froze it; an in-process fd cycle silently killed the host's event
//! source. Every one of those classes is a row here now.

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub mod persona;
pub mod spec;

pub use persona::{AnswerPolicy, TerminalPersona};
pub use spec::{apply, ConformanceSpec, SpecError};

/// A VT query a guest may send to its host. The catalog mirrors what the
/// fleet's guests actually emit (reedline, frost-zle, escriba-tui,
/// full-screen TUIs) — extend it when a new query class appears in a real
/// transcript, not speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VtQuery {
    /// DSR-6 `ESC[6n` — cursor position report. THE liveness-critical one.
    CursorPosition,
    /// DSR-5 `ESC[5n` — device status.
    DeviceStatus,
    /// DA1 `ESC[c` — primary device attributes.
    PrimaryDeviceAttributes,
    /// XTVERSION `ESC[>0q`.
    TerminalVersion,
    /// OSC 10 — foreground color query.
    OscForeground,
    /// OSC 11 — background color query.
    OscBackground,
}

impl VtQuery {
    /// The wire bytes a guest emits for this query.
    #[must_use]
    pub fn wire(&self) -> &'static [u8] {
        match self {
            Self::CursorPosition => b"\x1b[6n",
            Self::DeviceStatus => b"\x1b[5n",
            Self::PrimaryDeviceAttributes => b"\x1b[c",
            Self::TerminalVersion => b"\x1b[>0q",
            Self::OscForeground => b"\x1b]10;?\x1b\\",
            Self::OscBackground => b"\x1b]11;?\x1b\\",
        }
    }

    /// Scan `haystack` from `from` for the next query occurrence of any
    /// kind. Returns `(query, offset_after_match)`. Rolling-scan safe:
    /// callers keep their own cursor so split escapes across reads are
    /// re-scanned once complete (the E1-harness fragility, codified).
    #[must_use]
    pub fn scan(haystack: &[u8], from: usize) -> Option<(Self, usize)> {
        const CATALOG: [VtQuery; 6] = [
            VtQuery::CursorPosition,
            VtQuery::DeviceStatus,
            VtQuery::PrimaryDeviceAttributes,
            VtQuery::TerminalVersion,
            VtQuery::OscForeground,
            VtQuery::OscBackground,
        ];
        let mut best: Option<(Self, usize, usize)> = None; // (q, start, end)
        for q in CATALOG {
            let w = q.wire();
            if let Some(pos) = find(&haystack[from..], w) {
                let start = from + pos;
                if best.is_none() || start < best.unwrap().1 {
                    best = Some((q, start, start + w.len()));
                }
            }
        }
        best.map(|(q, _, end)| (q, end))
    }
}

/// A host's answer to a [`VtQuery`]. Typed, never a raw format string at
/// call sites — `wire()` is the single serializer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VtAnswer {
    /// `ESC[{row};{col}R` (1-based).
    CursorPosition { row: u16, col: u16 },
    /// `ESC[0n` — terminal OK.
    DeviceOk,
    /// `ESC[?1;2c` — VT100 with AVO; the conventional minimal DA1.
    Vt100WithAvo,
    /// Raw passthrough for answers whose shape is host-specific
    /// (XTVERSION strings, OSC color replies).
    Raw(Vec<u8>),
}

impl VtAnswer {
    /// The conventional answer for a query, for personas that just want
    /// to behave like a plain healthy terminal.
    #[must_use]
    pub fn conventional(query: VtQuery, row: u16, col: u16) -> Self {
        match query {
            VtQuery::CursorPosition => Self::CursorPosition { row, col },
            VtQuery::DeviceStatus => Self::DeviceOk,
            VtQuery::PrimaryDeviceAttributes => Self::Vt100WithAvo,
            VtQuery::TerminalVersion => Self::Raw(b"\x1bP>|espelho 0.1\x1b\\".to_vec()),
            VtQuery::OscForeground => Self::Raw(b"\x1b]10;rgb:ecec/efef/f4f4\x1b\\".to_vec()),
            VtQuery::OscBackground => Self::Raw(b"\x1b]11;rgb:2e2e/3434/4040\x1b\\".to_vec()),
        }
    }

    /// Wire bytes of this answer.
    #[must_use]
    pub fn wire(&self) -> Vec<u8> {
        match self {
            Self::CursorPosition { row, col } => {
                let mut out = Vec::with_capacity(12);
                out.extend_from_slice(b"\x1b[");
                push_u16(&mut out, *row);
                out.push(b';');
                push_u16(&mut out, *col);
                out.push(b'R');
                out
            }
            Self::DeviceOk => b"\x1b[0n".to_vec(),
            Self::Vt100WithAvo => b"\x1b[?1;2c".to_vec(),
            Self::Raw(bytes) => bytes.clone(),
        }
    }
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    let mut buf = [0u8; 5];
    let mut i = buf.len();
    let mut v = u32::from(v);
    if v == 0 {
        out.push(b'0');
        return;
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&buf[i..]);
}

/// The two conformance roles. A composition row is `(host, guest)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Owns the terminal side; must answer queries per the spec.
    Host,
    /// Runs inside the terminal; must stay live per the spec.
    Guest,
}

/// The mockable environment the conformance interpreter runs against.
/// L0/L1 harnesses implement this over a real PTY (master side); unit
/// tests implement it over in-memory duplex buffers with a typed clock.
pub trait TermEnv {
    type Error: std::fmt::Display;

    /// Write bytes toward the guest (i.e. onto the PTY master).
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Read available bytes from the guest, waiting at most `timeout`.
    /// `Ok(empty)` means timeout with no data — NOT end-of-stream.
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Self::Error>;
    /// Monotonic now, for latency assertions. Mockable so per-commit CI
    /// never depends on wall-clock (the typed-Clock rule).
    fn now(&self) -> Duration;
    /// Whether the guest process is still alive.
    fn guest_alive(&mut self) -> bool;
}

/// One observed query/answer exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exchange {
    pub query: VtQuery,
    /// `None` = persona policy chose not to answer (Mute).
    pub answered: Option<VtAnswer>,
    /// Time from query observation to answer write.
    pub latency: Duration,
    /// Offset into the transcript where the query ended.
    pub at: usize,
}

/// The outcome of driving one conformance scenario. The matrix
/// aggregates these — per the verification-matrix discipline, a missing
/// row is a failure, and failures aggregate before asserting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceOutcome {
    pub role: Role,
    pub exchanges: Vec<Exchange>,
    pub transcript_len: usize,
    pub guest_alive_at_end: bool,
    /// Marker round-trips observed (the "command executed" proof —
    /// liveness is NEVER asserted on aliveness alone; the 2026-06-10
    /// false-verification rule).
    pub markers_seen: usize,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_wire_roundtrip_via_scan() {
        for q in [
            VtQuery::CursorPosition,
            VtQuery::DeviceStatus,
            VtQuery::PrimaryDeviceAttributes,
            VtQuery::TerminalVersion,
            VtQuery::OscForeground,
            VtQuery::OscBackground,
        ] {
            let mut stream = b"prefix".to_vec();
            stream.extend_from_slice(q.wire());
            stream.extend_from_slice(b"suffix");
            let (found, end) = VtQuery::scan(&stream, 0).expect("query found");
            assert_eq!(found, q);
            assert_eq!(end, 6 + q.wire().len());
        }
    }

    #[test]
    fn scan_returns_earliest_query() {
        let mut stream = Vec::new();
        stream.extend_from_slice(VtQuery::DeviceStatus.wire());
        stream.extend_from_slice(VtQuery::CursorPosition.wire());
        let (first, end) = VtQuery::scan(&stream, 0).unwrap();
        assert_eq!(first, VtQuery::DeviceStatus);
        let (second, _) = VtQuery::scan(&stream, end).unwrap();
        assert_eq!(second, VtQuery::CursorPosition);
    }

    #[test]
    fn cpr_answer_wire_shape() {
        let a = VtAnswer::CursorPosition { row: 24, col: 1 };
        assert_eq!(a.wire(), b"\x1b[24;1R");
        let a = VtAnswer::CursorPosition { row: 150, col: 40 };
        assert_eq!(a.wire(), b"\x1b[150;40R");
    }
}
