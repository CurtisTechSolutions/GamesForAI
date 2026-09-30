# UCI boundary

`planning_position` reconstructs a rules-validated position from the acting
player's observation and verifies the supplied legal actions. `position_command`
retains the initial FEN and move history for repetition, with strict token/size
validation. No game crate is imported.

The parser handles negotiation, declared option bounds, typed centipawn/mate
scores, bound markers, WDL, MultiPV, principal variations and bestmove. It limits
line/PV sizes and rejects embedded control characters. Settings emit only
allowlisted numeric/boolean options; no arbitrary commands, paths or file options
are accepted. Pooled-engine strength options are reset between requests.

Protocol reference: [Stockfish UCI documentation](https://official-stockfish.github.io/docs/stockfish-wiki/UCI-Protocol-and-Stockfish-Commands.html).
The process pool has 1–4 lazy workers. Each request includes startup in its wall
budget, polls host cancellation at least every 10 ms, and rejects work when all
slots are busy. A failure kills and reaps the worker; the next request starts a
replacement. Stdin and stdout run on bounded worker channels so neither a blocked
pipe nor an output flood can hold the host indefinitely. Output is limited to
8 KiB per line, 128 queued lines, and 4 MiB/20,000 lines per phase.

Production launch requires Linux, bubblewrap with unprivileged user namespaces,
and util-linux prlimit. It fails closed when these are unavailable. The sandbox
has isolated network/PID/user namespaces, no capabilities, no host environment,
no home/store/socket mounts, and a read-only root and runtime mounts. Engine
binaries must include their evaluation data. Defaults limit each process to
2 GiB address space and 120 lifetime CPU seconds; hitting that CPU limit recycles
the process. The per-request wall limit also covers negotiation and options.
Do not configure user-writable engines or runtime directories.

The pool returns typed, bounded protocol output. Its caller must reconstruct the
position from the authorized observation and validate selected moves and PVs
through the game before returning them or updating a match.

Validation: fake engines exercise crash recovery, process reuse, cancellation,
busy admission, hung searches, blocked stdin and malformed/oversized output.
The dedicated UCI workflow runs a real Stockfish search through the production
sandbox. No production switch disables isolation.
