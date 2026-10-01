# Versioned position sets

Each `<name>@<version>.jsonl` has a matching manifest containing the game, source, license, count, and SHA-256 of the exact UTF-8 JSONL bytes (including final newline). Published versions are immutable; changes require a new version. Python wheels embed these exact files in the native module.

Load a bundled set with `gamesforai.PositionSet.load("chess-endgames-basic@1")`. Load your own data with `PositionSet.from_files(manifest_path, jsonl_path)`. Every entry is validated using its game engine before the set becomes available. Invalid entries, duplicate IDs or JSON keys, and content-hash mismatches reject the entire set.

The starter sets contain six elementary chess endgames and seven immediate-win Connect Four positions. Connect Four answers are verified by playing the specified action and checking the terminal return. Chess entries describe study positions without claiming engine-evaluated outcomes. Generated data is dedicated under [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/); source code retains the repository license.

Entries support `id`, `position`, optional `config`, `tags`, `difficulty`, `rating`, `expected`, and `source`. Preserve imported source IDs and licenses. Expected answers are dataset metadata for trusted training/evaluation code and must not be added to policy observations. These are public starter datasets; no hidden benchmark split is bundled.
