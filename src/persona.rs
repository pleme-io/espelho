//! Typed terminal personas — mock HOSTS that drive real guests.
//!
//! A persona is the test-side implementation of the host role: it owns the
//! PTY master (via a [`crate::TermEnv`]) and answers (or refuses to answer)
//! the guest's VT queries per a typed [`AnswerPolicy`]. The catalog mirrors
//! the real failure modes the fleet has actually hit — every variant is an
//! incident class, not a speculation.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{VtAnswer, VtQuery};

/// How a persona treats guest queries. Every variant maps to a lived
/// incident class (2026-06-10 lineage in the crate docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnswerPolicy {
    /// Answer every query after `latency`. Real hosts answer with latency
    /// (mado's engate→VT→send_keys loop is tens of ms) — and the CPR
    /// answer-loss race only triggers when the answer lands after the
    /// guest's filtered read window. Zero-latency personas miss it.
    Answer { latency: Duration },
    /// Never answer — the hostile/minimal-terminal class. Guests must stay
    /// fully usable (slow is acceptable; dead or input-deaf is not).
    Mute,
    /// Answer with the reply fragmented across two writes separated by
    /// `gap` — escape sequences are not atomic on the wire.
    SplitReply { gap: Duration },
    /// Answer the first `n` queries, then go mute — the "host died
    /// mid-session" class (mado's attach thread exiting, a daemon restart).
    AnswerThenMute { n: u32, latency: Duration },
}

/// A persona = a policy plus the cursor coordinates it reports. Pure data;
/// the interpreter in [`crate::spec`] does the driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalPersona {
    pub policy: AnswerPolicy,
    /// Reported cursor row/col for CPR answers (1-based).
    pub report_row: u16,
    pub report_col: u16,
}

impl TerminalPersona {
    /// A plain healthy terminal with realistic latency.
    #[must_use]
    pub fn healthy() -> Self {
        Self {
            policy: AnswerPolicy::Answer {
                latency: Duration::from_millis(50),
            },
            report_row: 24,
            report_col: 1,
        }
    }

    /// The hostile class: never answers anything.
    #[must_use]
    pub fn mute() -> Self {
        Self {
            policy: AnswerPolicy::Mute,
            report_row: 24,
            report_col: 1,
        }
    }

    /// What this persona answers for `query` given it is the `seen`-th
    /// query observed (1-based), or `None` for silence.
    #[must_use]
    pub fn answer_for(&self, query: VtQuery, seen: u32) -> Option<VtAnswer> {
        let answer = VtAnswer::conventional(query, self.report_row, self.report_col);
        match self.policy {
            AnswerPolicy::Answer { .. } | AnswerPolicy::SplitReply { .. } => Some(answer),
            AnswerPolicy::Mute => None,
            AnswerPolicy::AnswerThenMute { n, .. } => (seen <= n).then_some(answer),
        }
    }

    /// The write latency this persona imposes before answering.
    #[must_use]
    pub fn latency(&self) -> Duration {
        match self.policy {
            AnswerPolicy::Answer { latency } | AnswerPolicy::AnswerThenMute { latency, .. } => {
                latency
            }
            AnswerPolicy::SplitReply { gap } => gap,
            AnswerPolicy::Mute => Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_never_answers() {
        let p = TerminalPersona::mute();
        assert_eq!(p.answer_for(VtQuery::CursorPosition, 1), None);
        assert_eq!(p.answer_for(VtQuery::DeviceStatus, 99), None);
    }

    #[test]
    fn answer_then_mute_cuts_over() {
        let p = TerminalPersona {
            policy: AnswerPolicy::AnswerThenMute {
                n: 2,
                latency: Duration::ZERO,
            },
            report_row: 10,
            report_col: 5,
        };
        assert!(p.answer_for(VtQuery::CursorPosition, 1).is_some());
        assert!(p.answer_for(VtQuery::CursorPosition, 2).is_some());
        assert!(p.answer_for(VtQuery::CursorPosition, 3).is_none());
    }

    #[test]
    fn healthy_reports_declared_position() {
        let p = TerminalPersona::healthy();
        assert_eq!(
            p.answer_for(VtQuery::CursorPosition, 1),
            Some(VtAnswer::CursorPosition { row: 24, col: 1 })
        );
    }
}
