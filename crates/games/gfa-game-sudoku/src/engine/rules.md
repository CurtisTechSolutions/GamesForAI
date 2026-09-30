Fill each row, column and box with every digit from 1 through the board size exactly once.
Boards are 4×4 (2×2 boxes), 6×6 (2×3 boxes), or 9×9 (3×3 boxes).
Seat 0 is the only player. Starting givens are fixed, and every puzzle has exactly one solution.

Rows run from top to bottom, columns from left to right, both starting at 1.
Place a digit with `r3c5=7` or `{"type":"place","row":3,"col":5,"digit":7}`.
Erase a mutable cell with `r3c5=0` or `{"type":"erase","row":3,"col":5}`.
On an empty mutable cell, `r3c5+7` adds a candidate note and `r3c5-7` removes it.
Notes are optional, free, and do not count towards max_moves or scoring. Filled cells have no notes.

Silent mode permits any placement; the mistake count is disclosed only when the grid is full or play ends.
Rule-check mode rejects a placement that duplicates a row, column or box digit and names a conflicting cell.
Solution-check mode accepts wrong digits and ends unsuccessfully after the configured number of mistakes.
Repeating the same value or note is not a move. Rejected actions do not change the board or counters.

The unique solution earns +1. Reaching a mistake limit earns 0.
A placement/erasure cap (default 200) truncates play with return 0.
A separate 50,000-action safety cap also truncates a stream containing excessive free notes.
The board can be corrected after an incorrect full grid if a cap has not ended play.

Generation uses the match seed and records a technique grade: easy requires singles;
medium needs pairs or locked candidates; hard needs triples or X-wing; expert needs search.
No answer grid is sent in observations. This single-player game has no opponent difficulty ladder.
