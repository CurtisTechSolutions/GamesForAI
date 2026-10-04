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
