;; espelho :: term-conformance — the authored spec (triplet rule #2).
;;
;; One contract, two roles, consumed from both sides of the PTY:
;;   host  — owns the terminal; answers VT queries per policy
;;   guest — runs inside a terminal; stays LIVE regardless of host behavior
;; "Together and apart": each side conforms alone; every host × guest pair
;; is then a mechanical composition row with the same invariants.

(defterm-conformance
  :name "fleet-terminal-conformance"

  ;; The query catalog — mirrors what fleet guests actually emit
  ;; (reedline / frost-zle / escriba-tui). Extend from real transcripts,
  ;; never speculatively.
  :queries
    ((:kind :cursor-position  :wire "\x1b[6n"    :liveness-critical #t)
     (:kind :device-status    :wire "\x1b[5n"    :liveness-critical #f)
     (:kind :primary-da       :wire "\x1b[c"     :liveness-critical #f)
     (:kind :terminal-version :wire "\x1b[>0q"   :liveness-critical #f)
     (:kind :osc-foreground   :wire "\x1b]10;?"  :liveness-critical #f)
     (:kind :osc-background   :wire "\x1b]11;?"  :liveness-critical #f))

  ;; Canonical personas — every one is a LIVED incident class (2026-06-10):
  ;;   healthy        — answers at realistic latency (the race window)
  ;;   mute           — never answers (hostile/minimal terminal)
  ;;   split-reply    — fragments the answer (escapes are not atomic)
  ;;   answer-then-mute — host dies mid-session (attach thread exit)
  :personas
    ((:name :healthy          :policy (:answer :latency-ms 50))
     (:name :mute             :policy :mute)
     (:name :split-reply      :policy (:split-reply :gap-ms 10))
     (:name :answer-then-mute :policy (:answer-then-mute :n 2 :latency-ms 20)))

  ;; HOST invariants (mado Terminal, tear PaneGrid, any future host):
  ;;   answer-every-query   — ∀ byte-prefix of any real transcript, a
  ;;                          liveness-critical query fed after the prefix
  ;;                          is answered (the APC-black-hole class-killer)
  ;;   answer-latency       — answers within :budget-ms of the query
  ;;                          completing on the wire
  :host-invariants
    ((:name :answer-every-query :budget-ms 100)
     (:name :answer-latency     :budget-ms 100))

  ;; GUEST invariants (frost, frostmourne, escriba-tui):
  ;;   never-fatal     — no persona may kill the guest (E2)
  ;;   no-freeze       — a command round-trips under EVERY persona; echo
  ;;                     alone is not execution: marker count ≥ 2 (E4/E5)
  ;;   mode-restore    — full-screen guests (escriba-tui) restore the
  ;;                     terminal (leave alt-screen, cooked mode) on exit
  :guest-invariants
    ((:name :never-fatal  :applies-to :all-personas)
     (:name :no-freeze    :applies-to :all-personas :marker-min 2
            :note "mute personas may be slow (CPR timeouts) — budget
                   generously; dead or input-deaf is the failure")
     (:name :mode-restore :applies-to (:healthy) :guests (:fullscreen)))

  ;; The composition matrix — "together": every host × guest row runs the
  ;; guest invariants with the real host in place of a persona.
  :compositions
    ((:host :pty-persona :guest :frost)        ; L0 (frost repo)
     (:host :pty-persona :guest :frostmourne)  ; L0 + rc row
     (:host :mado-vt     :guest :frostmourne)  ; L1 (mado repo)
     (:host :mado-vt     :guest :escriba-tui)  ; L1/L2 full-screen row
     (:host :tear-grid   :guest :frostmourne)  ; M3 third site
     (:host :pty-persona :guest :escriba-tui))) ; escriba apart
