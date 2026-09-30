White (seat 0) moves first; Black is seat 1. Alternate one legal move. Capture an enemy piece by moving onto its square; never capture a king or leave your king in check.

Files a–h run left to right from White's side, ranks 1–8 bottom to top. Rooks move along ranks/files; bishops diagonally; queens combine both. Sliding pieces cannot jump. Knights jump two squares along one axis and one along the other. Kings move one square. Pawns advance one square into an empty cell (optionally two from their original rank when both cells are empty), capture one square diagonally forward, and promote on the last rank to queen, rook, bishop or knight.

Castling uses a king and its original rook, provided neither has moved, the intervening squares are empty, and the king is not in check and does not cross or enter check. The king moves two files toward the rook and the rook moves to the crossed square. Use e1g1/e1c1 for White, e8g8/e8c8 for Black. Castling rights in imported FEN must be valid.

En passant captures a pawn that just advanced two squares, on the immediately following move only. It is illegal if removing the pawns exposes your king. Promotion requires a lowercase q, r, b or n suffix, including when capturing.

Checkmate wins (+1 winner, −1 loser). Stalemate and insufficient mating material draw (0 each). Three occurrences of the same position, or 100 halfmoves without a pawn move/capture, permit a draw claim by the player to move. Use claim_draw for the current position or claim_draw:<UCI> for a legal intended move establishing the claim; a successful claim ends without moving. Five occurrences and 150 such halfmoves draw automatically, with checkmate taking precedence. Repetition includes side to move, castling rights and legally available en passant. Claims are listed alongside legal moves.

The configured max_plies cap truncates play with zero returns and is distinct from a rules draw. Resignation, draw agreement and wall-clock time controls are supplied by the match service.

Examples:
1. From the normal start: e2e4, e7e5, g1f3. SAN equivalents are e4, e5, Nf3, but input must be UCI.
2. From 4k3/P7/8/8/8/8/7p/4K3 w - - 0 1, a7a8n underpromotes to a knight; a7a8 is illegal.
3. Repeat g1f3, g8f6, f3g1, f6g8 twice from the normal start. White can claim_draw; another ordinary move remains allowed.

FEN describes one board, not its prior repetition history. Full played-state JSON exports preserve the starting FEN and all actions. Importing only FEN intentionally starts new repetition history. Validation checks kings, checks, pawns, castling and en-passant consistency through shakmaty; it does not attempt a full retrograde reachability proof for arbitrary composed positions.

References: [FIDE Laws, articles 3, 5 and 9](https://handbook.fide.com/chapter/E012023); [shakmaty](https://docs.rs/shakmaty/0.30.1/shakmaty/).
