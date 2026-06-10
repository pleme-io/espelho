# espelho (mirror)

Typed terminal-conformance contract: **hosts** answer VT queries, **guests**
survive any host. The `(defterm-conformance …)` triplet for the
mado / frost / frostmourne / escriba together-and-apart matrix.

- `src/lib.rs` — typed border: `VtQuery`, `VtAnswer`, `Role`, `TermEnv`
- `src/persona.rs` — incident-derived host personas (healthy / mute / split-reply / answer-then-mute)
- `src/spec.rs` — the conformance interpreter `apply(spec, env)` over a mockable `TermEnv`
- `specs/term-conformance.lisp` — the authored spec (queries, personas, invariants, composition matrix)

Lineage: the 2026-06-10 fleet incidents (CPR answer-loss race; kqueue-deletion
input wedge; APC black hole). Every persona and invariant is a lived failure
class. See `mado/docs/INTEGRATION-TESTING.md`.
