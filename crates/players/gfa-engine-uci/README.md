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
Process pooling, hard limits and sandboxing are a separate implementation unit;
this crate currently performs no I/O and does not launch a binary.
