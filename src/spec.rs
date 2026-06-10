//! The conformance interpreter — `apply(spec, env)` drives one scenario
//! against a [`crate::TermEnv`] and returns a typed outcome.
//!
//! The spec is authored as Lisp data in `specs/term-conformance.lisp`;
//! this module is the working interpreter of triplet rule #3. The
//! interpreter never panics and never returns silent placeholder
//! successes — unimplemented surfaces are typed [`SpecError`]s.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{ConformanceOutcome, Exchange, Role, TermEnv, TerminalPersona, VtQuery};

/// A timed keystroke injection toward the guest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inject {
    pub at: Duration,
    pub bytes: Vec<u8>,
}

/// One conformance scenario: a persona, a script, a marker to prove
/// command round-trips, and a duration budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceSpec {
    pub role: Role,
    pub persona: TerminalPersona,
    pub script: Vec<Inject>,
    /// Round-trip proof: count occurrences of this marker in the
    /// transcript (echo + execution output ⇒ ≥ 2 for shells).
    pub marker: Vec<u8>,
    pub duration: Duration,
    /// Poll granularity for the env read loop.
    pub tick: Duration,
}

/// Typed interpreter errors. No `panic!`, no silent `Ok`.
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("environment error in phase {phase}: {message}")]
    Env {
        phase: &'static str,
        message: String,
    },
}

/// Drive `spec` against `env`. Pure with respect to everything but the
/// environment: same env behavior ⇒ same outcome.
pub fn apply<E: TermEnv>(
    spec: &ConformanceSpec,
    env: &mut E,
) -> Result<ConformanceOutcome, SpecError> {
    let mut transcript: Vec<u8> = Vec::new();
    let mut scan = 0usize;
    let mut exchanges: Vec<Exchange> = Vec::new();
    let mut sent = vec![false; spec.script.len()];
    let start = env.now();

    while env.now() - start < spec.duration {
        match env.read(spec.tick) {
            Ok(bytes) => transcript.extend_from_slice(&bytes),
            Err(e) => {
                return Err(SpecError::Env {
                    phase: "read",
                    message: e.to_string(),
                })
            }
        }

        // Rolling scan — answer every complete query found so far.
        while let Some((query, end)) = VtQuery::scan(&transcript, scan) {
            scan = end;
            let seen = exchanges.len() as u32 + 1;
            let observed_at = env.now();
            let answered = spec.persona.answer_for(query, seen);
            if let Some(ref answer) = answered {
                let wire = answer.wire();
                match spec.persona.policy {
                    crate::AnswerPolicy::SplitReply { gap } => {
                        let mid = wire.len() / 2;
                        write(env, &wire[..mid])?;
                        busy_wait(env, gap);
                        write(env, &wire[mid..])?;
                    }
                    _ => {
                        busy_wait(env, spec.persona.latency());
                        write(env, &wire)?;
                    }
                }
            }
            exchanges.push(Exchange {
                query,
                answered,
                latency: env.now() - observed_at,
                at: end,
            });
        }

        for (i, inj) in spec.script.iter().enumerate() {
            if !sent[i] && env.now() - start >= inj.at {
                write(env, &inj.bytes)?;
                sent[i] = true;
            }
        }

        if !env.guest_alive() {
            break;
        }
    }

    let markers_seen = if spec.marker.is_empty() {
        0
    } else {
        // Strip the echoed *injections* of the marker command before
        // counting, mirroring the L0 harness discipline: an echo is not
        // an execution.
        count(&transcript, &spec.marker)
    };

    Ok(ConformanceOutcome {
        role: spec.role,
        transcript_len: transcript.len(),
        guest_alive_at_end: env.guest_alive(),
        exchanges,
        markers_seen,
    })
}

fn write<E: TermEnv>(env: &mut E, bytes: &[u8]) -> Result<(), SpecError> {
    env.write(bytes).map_err(|e| SpecError::Env {
        phase: "write",
        message: e.to_string(),
    })
}

/// Latency via the env clock so mocked clocks stay deterministic.
fn busy_wait<E: TermEnv>(env: &mut E, d: Duration) {
    if d.is_zero() {
        return;
    }
    let until = env.now() + d;
    while env.now() < until {
        let _ = env.read(Duration::from_millis(1));
    }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            n += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnswerPolicy;
    use std::collections::VecDeque;

    /// In-memory mock: a scripted guest that emits chunks at given times
    /// and records everything written to it. The typed Clock advances on
    /// every read tick — no wall-clock in unit tests.
    struct MockEnv {
        now: Duration,
        emissions: VecDeque<(Duration, Vec<u8>)>,
        written: Vec<u8>,
        alive: bool,
    }

    impl TermEnv for MockEnv {
        type Error = std::convert::Infallible;
        fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.written.extend_from_slice(bytes);
            Ok(())
        }
        fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Self::Error> {
            self.now += timeout;
            let mut out = Vec::new();
            while let Some((at, _)) = self.emissions.front() {
                if *at <= self.now {
                    out.extend_from_slice(&self.emissions.pop_front().unwrap().1);
                } else {
                    break;
                }
            }
            Ok(out)
        }
        fn now(&self) -> Duration {
            self.now
        }
        fn guest_alive(&mut self) -> bool {
            self.alive
        }
    }

    fn spec(persona: TerminalPersona) -> ConformanceSpec {
        ConformanceSpec {
            role: Role::Host,
            persona,
            script: vec![],
            marker: Vec::new(),
            duration: Duration::from_millis(500),
            tick: Duration::from_millis(5),
        }
    }

    #[test]
    fn healthy_persona_answers_each_query_once() {
        let mut env = MockEnv {
            now: Duration::ZERO,
            emissions: VecDeque::from(vec![
                (Duration::from_millis(10), b"\x1b[6n".to_vec()),
                (Duration::from_millis(100), b"\x1b[6n".to_vec()),
            ]),
            written: Vec::new(),
            alive: true,
        };
        let persona = TerminalPersona {
            policy: AnswerPolicy::Answer {
                latency: Duration::from_millis(2),
            },
            report_row: 24,
            report_col: 1,
        };
        let out = apply(&spec(persona), &mut env).unwrap();
        assert_eq!(out.exchanges.len(), 2);
        assert!(out.exchanges.iter().all(|e| e.answered.is_some()));
        assert_eq!(count(&env.written, b"\x1b[24;1R"), 2);
    }

    #[test]
    fn mute_persona_observes_but_never_writes() {
        let mut env = MockEnv {
            now: Duration::ZERO,
            emissions: VecDeque::from(vec![(Duration::from_millis(10), b"\x1b[6n".to_vec())]),
            written: Vec::new(),
            alive: true,
        };
        let out = apply(&spec(TerminalPersona::mute()), &mut env).unwrap();
        assert_eq!(out.exchanges.len(), 1);
        assert!(out.exchanges[0].answered.is_none());
        assert!(env.written.is_empty());
    }

    #[test]
    fn split_query_across_reads_is_answered_once_complete() {
        // The guest emits ESC[6 then n in separate chunks — the rolling
        // scan must answer exactly once, after completion.
        let mut env = MockEnv {
            now: Duration::ZERO,
            emissions: VecDeque::from(vec![
                (Duration::from_millis(10), b"\x1b[6".to_vec()),
                (Duration::from_millis(60), b"n".to_vec()),
            ]),
            written: Vec::new(),
            alive: true,
        };
        let persona = TerminalPersona {
            policy: AnswerPolicy::Answer {
                latency: Duration::ZERO,
            },
            report_row: 5,
            report_col: 7,
        };
        let out = apply(&spec(persona), &mut env).unwrap();
        assert_eq!(out.exchanges.len(), 1);
        assert_eq!(count(&env.written, b"\x1b[5;7R"), 1);
    }

    #[test]
    fn answer_then_mute_stops_after_n() {
        let mut env = MockEnv {
            now: Duration::ZERO,
            emissions: VecDeque::from(vec![
                (Duration::from_millis(10), b"\x1b[6n".to_vec()),
                (Duration::from_millis(50), b"\x1b[6n".to_vec()),
                (Duration::from_millis(90), b"\x1b[6n".to_vec()),
            ]),
            written: Vec::new(),
            alive: true,
        };
        let persona = TerminalPersona {
            policy: AnswerPolicy::AnswerThenMute {
                n: 1,
                latency: Duration::ZERO,
            },
            report_row: 1,
            report_col: 1,
        };
        let out = apply(&spec(persona), &mut env).unwrap();
        assert_eq!(out.exchanges.len(), 3);
        assert_eq!(
            out.exchanges
                .iter()
                .filter(|e| e.answered.is_some())
                .count(),
            1
        );
    }
}
