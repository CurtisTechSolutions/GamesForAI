# Local evaluation reports

Open **Reports** and choose the JSON file written by `gfa tournament`.
The viewer supports the runner's `format_version: 1` tournament reports.
It displays standings, reported Glicko-2 estimates and approximate 95% intervals,
per-game outcomes, failure codes, seeds and run configuration. Model and outcome
filters combine with search; game results render in pages of 25.

Files stay in the current browser tab. The viewer does not upload report data,
follow dataset paths, or turn native episode IDs into server replay links.
Keep the source JSON file to reopen it after a reload. Reports from native
runner evaluations and the server's persisted match history are separate records.

Imports are limited to 8 MiB, 32 agents and 10,000 games. The parser checks
version, finite numbers, unique identities, counter/score consistency, and
per-game participant and outcome data. Unknown extra fields are not rendered.
An unreadable file leaves the previously opened report available.

The example is real native-engine output: Tic-Tac-Toe, seed 42, ten games per
pair between a first-legal-action Python policy, random, and minimax level 3.
The run ID and timestamp are fixed to make the fixture reproducible. It is
explicitly marked as example data; these ratings depend on this small schedule
and its default priors. The failure test fixture uses a factory that raises an
exception before each game.

## Compare model snapshots

Choose **Compare runs**, open two reports, and select a snapshot from each run.
The comparison shows each snapshot's full-run score, reported rating interval,
and results against each opponent. Snapshot names may differ between runs.

Score changes are shown per shared opponent only when the game, engine version,
game options and position-set checksum agree, and both matchups have the same
seeds, model seats, and starting-position IDs. Changes are descriptive percentage
points (run B minus run A), not significance tests. Excluded games remain visible,
and a matchup with no rated games has no score. Aggregate ratings depend on the
entire opponent mix and priors; no cross-run rating delta or ranking is inferred.

Different settings and unmatched schedules are explained in the comparison.
Reports without position-set provenance remain readable, but do not receive
score changes because their starting-position source cannot be compared.
Replacing a report with an invalid file preserves the previous report and
selected snapshot. Both reports stay in memory in the current tab.

The candidate browser-test fixture is another native Tic-Tac-Toe run with the
same seed and schedule as the example. It uses a last-legal-action Python policy
(example:policy-v2), so comparisons exercise measured differences rather than
handwritten ratings.
