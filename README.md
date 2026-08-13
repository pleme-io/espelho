# espelho (mirror)

Typed terminal-conformance contract: **hosts** answer VT queries, **guests**
survive any host. The `(defterm-conformance …)` triplet for the
mado / frost / frostmourne / escriba together-and-apart matrix.

- `src/lib.rs` — typed border: `VtQuery`, `VtAnswer`, `Role`, `TermEnv`
- `src/persona.rs` — incident-derived host personas (healthy / mute / split-reply / answer-then-mute)
- `src/spec.rs` — the conformance interpreter `apply(spec, env)` over a mockable `TermEnv`
- `specs/term-conformance.lisp` — the authored spec (queries, personas, invariants, composition matrix)

## Where this is going

espelho is the **seed of the conformance half** of
[`theory/NATURALIZE-TERMINAL.md`](https://github.com/pleme-io/theory/blob/main/NATURALIZE-TERMINAL.md),
and it is already the right shape — a typed border, a `(def…)` spec, an `apply`
interpreter over a mockable `TermEnv`, every persona derived from a lived
incident. What it is not is a **sequence catalog**: six `VtQuery` variants cover
the host/guest query-answer axis, which is one axis of a vocabulary with ~1000
members.

That doc's M3 widens this: every declared sequence that is a *query* would get an
espelho contract entry automatically, so the host/guest matrix covers the whole
vocabulary instead of the six incidents that happened to be survived. **Widening
espelho is the single most reusable move in that plan** — nothing else there
already exists in the right shape.

Tier: that work is **design**; M0 is green over ten cursor sequences
([`pleme-io/masume`](https://github.com/pleme-io/masume)) and nothing has touched
espelho yet.

Lineage: the 2026-06-10 fleet incidents (CPR answer-loss race; kqueue-deletion
input wedge; APC black hole). Every persona and invariant is a lived failure
class. See `mado/docs/INTEGRATION-TESTING.md`.
