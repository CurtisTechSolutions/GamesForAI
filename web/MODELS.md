# Model configurations in the browser

Open **Models** from the navigation or **Set up a model** from the library.
Name each frozen model snapshot, then choose a Python factory or an
OpenAI-compatible HTTP model server. Saved configurations stay in this browser;
download `agent.json` to use a configuration in the Python runner.

Python factories receive `seed`, `seat`, and your JSON options and return a
policy that selects a legal integer action index. HTTP configurations name
an API root, model identifier and optional environment variable for a credential.
The runner reads that variable at execution time. The form validates settings
without importing Python code or contacting a model server.

The evaluation builder uses the live game catalog and supports the runner's
sequential two-player games, balanced seats, reproducible seeds, random/minimax/
MCTS opponents, and optional JSONL or Parquet trajectory export. Download the
configuration into your working folder, install the indicated SDK extras from
your checkout, and run the generated command. The optional episode script
checks one model against random moves before a longer evaluation.

Runner evaluations use native environments and save local reports and datasets.
The browser's Live and History pages display matches persisted by the server.
See [the Python SDK](../python/README.md) for training interfaces and
[the tournament guide](../examples/tournament-cli.md) for reporting details.

Storage is bounded to 12 configurations and 128 KiB. An unreadable or blocked
browser store leaves configuration download available. Exported agent documents
use the existing runner schema and do not include ratings or calibration claims.
