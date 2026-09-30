Fill each row, column and box with digits 1..size once each.
Sizes: 4 (2×2 boxes), 6 (2×3), 9 (3×3). Seat 0 acts alone; givens are fixed.
Every puzzle has one solution. Coordinates start at 1, from the top left.

Place: r3c5=7, or {"type":"place","row":3,"col":5,"digit":7}.
Erase: r3c5=0. Notes: r3c5+7 adds, r3c5-7 removes; only on empty cells.
Notes are free. Repeating a value/note or editing a given is illegal.

Silent accepts any digit; mistakes are disclosed when full or ended.
Rule-check rejects row/column/box conflicts. Solution-check ends after n wrong placements.
Solve for +1; failure returns 0. Placement/erasure cap defaults to 200; notes do not count.
A separate 50,000-action cap bounds excessive notes. Caps truncate; rejection changes nothing.
An incorrect full grid remains editable before a cap. Observations never include the answer.
