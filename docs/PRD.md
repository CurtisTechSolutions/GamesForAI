# GamesForAI — Product Requirements Document

| | |
|---|---|
| **Status** | Draft v0.2 (changes in §20) |
| **Last updated** | 2026-09-30 |
| **License** | GPL-3.0 (repo license; compatible with bundling Stockfish) |
| **Stack** | Rust backend (engines, API, MCP, storage) · React + Phaser frontend · PostgreSQL / SQLite |

---

## 1. Summary

GamesForAI is a game library built as a **training and evaluation environment for AI systems**. It provides:

1. **Game engines** in Rust. Each game is a separate, deterministic crate behind one shared interface.
2. **One standardized API** exposed through three transports: REST/WebSocket for services, **MCP** for LLM agents, and in-process bindings (Rust and Python) for high-throughput reinforcement learning.
3. **Built-in opponents** for when no human is playing: strong external solvers such as Stockfish for chess, generic fallbacks (MCTS, minimax, random), and LLM players that play through a model provider's API.
4. **Persistent game history** in a database. Every match is an append-only event log that can be replayed, analyzed and exported as training data.
5. **A single React + Phaser web app** to browse games, play them, watch live matches and scrub through replays.
6. **Benchmark suites:** fixed, versioned sets of tasks with standard reports, so results can be compared across agents and over time.

The main design principle: **learn the API once, play any game.** An agent that can play tic-tac-toe through the API can play chess through the same calls, reading the rules, observations and legal actions that each game describes about itself.

## 2. Problem & motivation

- Existing RL environments (Gymnasium, PettingZoo, OpenSpiel) are Python- or C++-centric. Each has its own conventions, and none targets **LLM agents**, which need text observations, self-describing rules, readable error messages and tool-calling interfaces.
- LLM agent evaluations on games are usually one-off scripts. Results are not reproducible or comparable, and they are rarely stored in a form that can be reused as training data.
- Setting up a strong opponent (Stockfish, KataGo, a Connect Four solver) for every game is repetitive integration work.
- Researchers want to *see* what their agent is doing: watch it live, replay its games and inspect its reasoning.

## 3. Goals & non-goals

### Goals
- **G1 – Standardized interface:** every game implements the same trait and is reachable through the same endpoints and MCP tools, with no game-specific client code.
- **G2 – LLM/MCP friendly:** self-describing games through a per-game info route (rules, notation, schemas, generated examples), text and JSON observations, legal-action listings, and error messages that tell the agent how to recover.
- **G3 – Training friendly:** deterministic seeded engines, fast headless stepping, vectorized batch environments, self-play, curricula from any start position, stateless simulation for planning, and Gymnasium/PettingZoo-compatible wrappers.
- **G4 – Strong baselines:** pluggable opponents at configurable strength for every game, plus LLM players run by the server.
- **G5 – Full history:** every match is persisted, reproducible from its seed and actions, and exportable (JSONL/Parquet).
- **G6 – Visual tooling:** play, spectate live and replay any match in the browser.
- **G7 – Comparable evaluation:** versioned benchmark suites with fixed tasks, opponents and scoring, producing standard reports that can be compared across agents and over time.

### Non-goals (v1)
- Hosting model training or inference. GamesForAI is the *environment*, not the trainer. The built-in LLM player calls external model APIs; it does not host models.
- Real-money play, public matchmaking at internet scale, or anti-cheat for human competitive play.
- Video/3D games or pixel-observation Atari-style emulation. v1 targets turn-based board, card and puzzle games; real-time games are a later phase.
- A mobile-native app. The web app should be responsive, which is sufficient.

## 4. Users & primary use cases

| Persona | Needs | Primary interface |
|---|---|---|
| **LLM agent developer** | Connect Claude or another LLM to games through MCP, evaluate reasoning, collect transcripts | MCP server |
| **RL researcher** | Millions of steps per hour, self-play, deterministic replays, gym-style API | Rust crate / Python SDK (in-process), batch REST |
| **Evaluation engineer** | Run benchmark suites and agent-vs-baseline tournaments, get ratings and reproducible reports | REST API + CLI |
| **Human player / observer** | Play against AI, watch AI-vs-AI live, review replays | Web app |
| **Game contributor** | Add a new game without touching the server or frontend core | Rust `Game` trait + Phaser scene plugin |

**Key user stories**
- As an LLM agent, I can call `list_games`, then `get_game_info("chess")`, then `create_match` against Stockfish at level 5, and play to completion using only `get_state` and `make_move`.
- As an RL researcher, I can `pip install gamesforai`, create 1,024 parallel Connect Four environments in-process and train with any Gymnasium-compatible library.
- As an evaluator, I can run a 200-game round robin between three agent endpoints and two baselines, then view a leaderboard and download every game as JSONL.
- As a human, I can open a live match between two agents, watch the moves animate, see each agent's optional stated reasoning, and later scrub the replay move by move.
- As an LLM agent, I can check where a line of play leads with `simulate_moves` before committing to a move, when the match allows it.
- As an evaluator, I can run `sudoku-graded-v1` on three models with the built-in LLM player and compare the reports side by side.
- As an RL researcher, I can train on a curriculum of chess endgames by passing a position set to `reset()`.
- As a human reviewing a replay, I can branch at a mistake and play the position out against an engine.

## 5. System overview

```
                    ┌──────────────────────────── web/ (React + Phaser) ───────────────────────────┐
                    │ Lobby · Play · Live spectate · Replay viewer · Leaderboards · Agent inspector│
                    └───────────────▲──────────────────────────────▲───────────────────────────────┘
                                    │ REST                          │ WebSocket (live events)
┌── LLM agents ──┐   MCP (stdio /   │                               │
│ Claude, etc.   │── streamable ───►┌─────────────────── gfa-server (Rust, axum) ───────────────────┐
└────────────────┘   HTTP)          │  API v1 · MCP · Auth · Matches · Ratings · Benchmarks         │
┌── RL trainers ─┐   REST/batch     │                                                               │
│ Python/Rust    │─────────────────►│   ┌──────────── Game registry (dyn Game) ─────────────┐       │
└────────────────┘                  │   │ tictactoe · connect4 · chess · checkers · go · ...│       │
        │ in-process (PyO3 / crate) │   └───────────────────────────────────────────────────┘       │
        └──────────────────────────►│   ┌──────────── Opponent adapters ────────────────────┐       │
                                    │   │ Stockfish · KataGo · solvers · MCTS · LLM players │       │
                                    │   └───────────────────────────────────────────────────┘       │
                                    └───────────────┬───────────────────────────────────────────────┘
                                                    │ sqlx
                                        PostgreSQL (prod) / SQLite (local)
```

### 5.1 Repository layout (Cargo workspace + web app)

```
GamesForAI/
├── Cargo.toml                 # workspace
├── crates/
│   ├── gfa-core/              # Game trait, shared types, schemas, errors, RNG, text rendering helpers
│   ├── gfa-games/             # one sub-crate per game (independent, feature-gated)
│   │   ├── tictactoe/
│   │   ├── connect4/
│   │   ├── sudoku/
│   │   ├── chess/             # wraps `shakmaty` for move generation
│   │   └── ...
│   ├── gfa-opponents/         # Opponent trait + adapters (uci, gtp, mcts, minimax, random)
│   ├── gfa-llm/               # built-in LLM player: provider clients, prompts, transcripts
│   ├── gfa-store/             # sqlx repositories, migrations, export (JSONL/Parquet)
│   ├── gfa-server/            # axum HTTP + WebSocket API, match orchestration, auth
│   ├── gfa-mcp/               # MCP server (rmcp), maps tools onto gfa-core/gfa-server
│   ├── gfa-bench/             # benchmark suites: loader, runner, scoring, reports
│   ├── gfa-py/                # PyO3 bindings + Gymnasium/PettingZoo wrappers (maturin)
│   └── gfa-cli/               # `gfa` CLI: run server, play, tournaments, benchmarks, export
├── web/                       # React + Vite + Phaser app
│   └── src/games/<game_id>/   # one Phaser scene per game
├── schemas/                   # generated JSON Schemas + OpenAPI spec (source of truth for TS types)
├── positions/                 # position sets (JSONL) for curricula, puzzles and benchmarks
├── benchmarks/                # versioned benchmark suite definitions (public splits only)
├── docker/                    # Dockerfiles, compose (server, db, engines)
└── docs/
```

Each game is its own crate with no dependency on the server, database or other games. It can be tested, benchmarked and published on its own, and embedded directly in a training loop.

## 6. Functional requirements — Game engine layer

### 6.1 The `Game` trait (core contract)

All games implement one trait in `gfa-core`. Sketch:

```rust
pub trait Game: Send + Sync + 'static {
    type State: Clone + Serialize + DeserializeOwned;
    type Action: Clone + Serialize + DeserializeOwned + Eq + Hash;
    type Config: Default + Serialize + DeserializeOwned + JsonSchema;

    /// Static metadata: id, name, player counts, properties, rules text, schemas.
    fn spec() -> GameSpec;

    fn new_initial_state(config: &Self::Config, seed: u64) -> Self::State;

    /// Players who must act now (empty when terminal; >1 for simultaneous moves).
    fn current_players(state: &Self::State) -> Vec<PlayerId>;
    fn legal_actions(state: &Self::State, player: PlayerId) -> Vec<Self::Action>;
    fn apply(state: &mut Self::State, player: PlayerId, action: &Self::Action) -> Result<StepEvents, GameError>;

    fn is_terminal(state: &Self::State) -> bool;
    /// Per-player returns; final at terminal, running value otherwise.
    fn returns(state: &Self::State) -> Vec<f64>;

    /// Observation from one player's point of view (hides hidden information).
    fn observe(state: &Self::State, viewer: Viewer) -> Observation;

    // Action encodings — every action has all three forms.
    fn action_to_string(state: &Self::State, action: &Self::Action) -> String; // e.g. "e2e4"
    fn action_from_string(state: &Self::State, s: &str) -> Result<Self::Action, GameError>;
    fn action_to_index(action: &Self::Action) -> u32;          // fixed discrete space
    fn action_from_index(state: &Self::State, idx: u32) -> Result<Self::Action, GameError>;

    // Positions (FR-E10): write any state in the game's standard notation and read one back.
    fn state_to_notation(state: &Self::State) -> String;       // FEN, 81-char Sudoku grid, ...
    fn state_from_notation(config: &Self::Config, s: &str) -> Result<Self::State, GameError>;
}
```

A type-erased `DynGame` wrapper (JSON in, JSON out) lets the server, MCP layer and Python bindings treat every game identically through a `GameRegistry`.

**Requirements**
- **FR-E1 Determinism:** given `(game_id, engine_version, config, seed, action sequence)`, the resulting state is bit-for-bit reproducible. All randomness (card shuffles, dice) comes from a seeded, serializable RNG stored in the state. Chance events are recorded in the event log.
- **FR-E2 Game properties** declared in `GameSpec`: `num_players` (min/max), `turn_structure` (sequential | simultaneous), `information` (perfect | imperfect), `stochastic` (bool), `max_game_length`, `reward_range`, `action_space_size`.
- **FR-E3 Hidden information:** `observe(state, Viewer::Player(p))` must never leak other players' private information. `Viewer::Spectator` and `Viewer::Omniscient` (replays, admin) are separate views. Tested with property tests.
- **FR-E4 Three observation encodings** per observation:
  - `text`: a readable board plus a short status line, designed to fit into an LLM prompt (e.g. ASCII board with coordinates, "White to move, move 12").
  - `json`: structured state that follows the game's published observation schema (the frontend renders from this too).
  - `tensor` (optional, feature `tensor`): fixed-shape `f32` planes plus declared `shape`, for neural networks.
- **FR-E5 Three action encodings:** structured JSON (schema-validated), canonical string notation (UCI for chess, column number for Connect Four, etc.), and a fixed discrete index plus a **legal-action mask**, for RL.
- **FR-E6 Rules as data:** `GameSpec.rules_markdown` holds complete, concise rules, win conditions, notation explanation and 1–3 worked examples. It is the `rules` section of the game info route (§8.4).
- **FR-E7 Termination:** games declare a max length. Draw rules (threefold repetition, 50-move rule, etc.) are enforced by the engine. Truncation is distinct from termination in the step result.
- **FR-E8 Performance:** engines are allocation-light. Targets are listed in §13.
- **FR-E9 Conformance test kit:** `gfa-core::testing::conformance::<G>()` runs random playouts to check determinism, serialization round-trips, legal-action/apply consistency, index/string encoding bijections, observation schema validity, position notation round-trips and hidden-info non-leakage. Every game must pass it in CI.
- **FR-E10 Positions:** every game can write any state in its standard notation and read one back with full validation: FEN for chess, SGF for Go, the 81-character grid for Sudoku, and the JSON state for games without a common notation. Import rejects impossible positions with `INVALID_POSITION` and a plain-English hint (e.g. "Black has no king", "column 3 has a floating piece"). Finished positions can be imported for analysis but can't start a match. Imported positions get a fresh seeded RNG for later chance events.

### 6.2 Game catalog

| Phase | Game | Players | Info | Why it's included | Reference opponent |
|---|---|---|---|---|---|
| MVP | Tic-Tac-Toe | 2 | Perfect | Smoke test, trivially solvable | Perfect minimax |
| MVP | Connect Four | 2 | Perfect | Solved game, graded difficulty | Bitboard negamax solver (Pons-style), depth-limited for levels |
| MVP | Chess | 2 | Perfect | Flagship, strong engine available | **Stockfish** (UCI), Skill Level 0–20 / UCI_Elo |
| MVP | Sudoku | 1 | Perfect | Pure constraint reasoning, cheap to build, exercises the single-player path from the start | Constraint-propagation + dancing-links solver (§6.3) |
| P2 | Checkers (English draughts) | 2 | Perfect | Classic, solved | Alpha-beta engine |
| P2 | Othello | 2 | Perfect | Medium complexity | Edax (GPL) or built-in alpha-beta |
| P2 | Go (9×9, 13×13, 19×19) | 2 | Perfect | Deep planning | **KataGo** (GTP), GNU Go fallback |
| P2 | 2048 / Sokoban | 1 | Perfect, stochastic / deterministic | Single-agent planning | Expectimax / solver |
| P3 | Heads-up Limit Texas Hold'em | 2 | Imperfect, stochastic | Bluffing, hidden info | CFR-based bot |
| P3 | Battleship | 2 | Imperfect | Inference under uncertainty | Probability-density heuristic |
| P3 | Liar's Dice / Kuhn Poker | 2+ | Imperfect | Small game-theory testbeds | Exact Nash / CFR |
| P3 | Hanabi | 2–5 | Imperfect, cooperative | Cooperation, theory of mind | Rule-based bots |

### 6.3 Sudoku (first single-player game)

Sudoku is the first single-player game and the first puzzle, so it proves early that the `Game` trait, API, info route and frontend handle one-seat games, where there is no opponent and the "result" is solving the puzzle. It is also a clean LLM reasoning benchmark: each puzzle has exactly one solution, every move can be checked, and difficulty can be graded precisely.

- **Puzzle generation:** puzzles are generated from the match seed, so the same seed always gives the same puzzle. Every generated puzzle has **exactly one solution** (checked by the solver). Puzzles can also be loaded from a given grid (`puzzle: "53..7...."`, 81 characters, `.` or `0` for empty), for benchmarks and imported datasets. The same grid is Sudoku's start-position notation (FR-E10).
- **Config:** `size` (4×4, 6×6 or 9×9; default 9), `difficulty` (`easy`, `medium`, `hard`, `expert`), `puzzle?` (a given grid), `mistake_policy` (see below), `allow_notes` (default true), `max_moves?`.
- **Difficulty grading:** difficulty is set by the hardest solving technique the solver needs, not by the number of clues: `easy` = singles only, `medium` = pairs and pointing, `hard` = triples, X-wing and similar, `expert` = chains or guessing. The grade and technique list are stored with the match.
- **Actions:**
  - `place` a digit: string `r3c5=7` (row, column, digit, 1-based), JSON `{"type": "place", "row": 3, "col": 5, "digit": 7}`.
  - `erase` a cell: `r3c5=0`.
  - `note` a candidate (if `allow_notes`): `r3c5+7` to add, `r3c5-7` to remove. Notes are free: they don't count as moves or affect scoring.
  - Discrete index space for 9×9: 729 place + 81 erase + 1,458 note actions. Legal actions exclude changing a given (starting) cell. Placements that break a row, column or box rule are still legal by default, so an agent can make mistakes that can be measured; see `mistake_policy`.
- **Mistake policy** (`mistake_policy`):
  - `silent` (default): any digit can be placed. Mistakes are only discovered when the grid is full.
  - `rule_check`: placing a digit that already appears in the same row, column or box is rejected as `ILLEGAL_ACTION`, with a hint naming the conflicting cell.
  - `solution_check:n`: placing a digit that differs from the solution counts as a mistake. The game ends as failed after `n` mistakes, as in common Sudoku apps.
- **Termination and rewards:** the game ends when the grid matches the solution (`outcome.reason: "solved"`, return +1) or on failure (`mistake_limit` or `max_moves`, return 0). Optional shaped reward: +1/81 for each correct placement and −1/81 for each wrong one. Metrics stored per match: moves, mistakes, erasures, time to solve, and the puzzle's difficulty grade.
- **Observation:** the text form is a grid with row and column numbers and box separators, empty cells shown as `.`, plus a status line ("41 cells left, 1 mistake"). The JSON form holds `grid` (81 digits, 0 = empty), `givens` (which cells are fixed), `notes` and `conflicts` (cells currently breaking a rule). The tensor form is 9 one-hot planes plus a givens plane.
- **Reference solver (instead of an opponent):** a constraint-propagation and dancing-links solver in Rust (solves any 9×9 in under 1 ms). It is used for generation, uniqueness checks, difficulty grading, a `hint` action via analysis (`POST /v1/analysis` returns the next logical step and the technique that finds it, if `allow_analysis`), and the "solved correctly" check. The `random` built-in player works as a baseline; there is no difficulty ladder because there is no opponent.
- **Info route:** the `opponents` section says the game is single-player, and `example_turns` shows a `place`, a `note` and a rejected change to a given cell.
- **Frontend:** a Phaser grid scene with givens in bold, notes drawn as small digits, conflicts highlighted, and keyboard input (arrow keys, 1–9, Backspace, `N` to toggle note mode). The replay viewer shows the grid filling in step by step, with mistakes marked.
- **Benchmarks:** the `sudoku-graded-v1` suite (§9) uses a fixed set of puzzles per difficulty and scores solve rate, mistakes per puzzle and moves per puzzle.

### 6.4 Position sets

A **position set** is a named, versioned list of positions for one game, stored as JSONL in `positions/`. Each line holds an `id`, the position in the game's notation (FR-E10), and optionally a `config`, `tags`, a difficulty or `rating`, an `expected` answer for puzzles (e.g. the solution line), and its `source`. Position sets feed curricula (§8.11), custom starts (§8.7) and benchmark tasks (§9).

| Set | Game | Source | Contents |
|---|---|---|---|
| `chess-puzzles-lichess` | Chess | Lichess puzzle database (CC0) | Tactics with solution lines, themes and puzzle ratings |
| `chess-endgames-basic` | Chess | Generated | Basic endgames (KQ-K, KR-K, KP-K, …) for curricula |
| `connect4-solved-positions` | Connect Four | Generated with the solver | Positions with their exact game-theoretic value and best moves |
| `sudoku-graded` | Sudoku | Generated with the solver | Unique-solution puzzles tagged with difficulty grade and techniques |

- **FR-P1** Every position is validated when a set is loaded. A set containing an invalid position fails to load.
- **FR-P2** A published version never changes. Sets are addressed as `<set>@<version>` and carry a content hash, which benchmark suites pin.
- **FR-P3** Each set records its source and license. Imports from outside sources keep their original ids so results can be traced back.

## 7. Functional requirements — Opponents (built-in players)

- **FR-O1 `Opponent` trait:** `choose_action(observation, legal_actions, clock) -> ActionChoice { action, info }`, where `info` can include eval score, principal variation, search depth and nodes. Opponents only see what a player is allowed to see.
- **FR-O2 Adapters:**
  - `uci`: generic UCI engine process manager, used for Stockfish (and Lc0 later). Supports `Skill Level`, `UCI_LimitStrength`/`UCI_Elo`, movetime/nodes/depth limits and `MultiPV` for analysis.
  - `gtp`: generic GTP adapter for Go engines (KataGo, GNU Go).
  - `builtin`: in-process Rust solvers (minimax/alpha-beta, MCTS, expectimax, CFR). Every game gets MCTS and `random` for free through the `Game` trait.
  - `remote_agent`: an external agent reachable via HTTP callback or WebSocket. This is how two user agents play each other through the server.
  - `llm`: the built-in LLM player (§7.1).
- **FR-O3 Standard difficulty ladder:** each opponent exposes `level: 1..=10`, mapped per engine (e.g. Stockfish UCI_Elo 1350→2850; Connect Four search depth). Levels are calibrated so that level *n+1* beats level *n* about 70% of the time. Calibration runs as a scheduled job and results are published in the game spec. LLM players have no levels; their strength is measured through ratings and benchmark results.
- **FR-O4 Engine process management:** engine processes are pooled and reused. Each has hard CPU-time, memory and wall-clock limits, is restarted when it crashes, and runs sandboxed (no network, read-only FS). Engine binaries ship in the Docker image and their paths are configurable.
- **FR-O5 Analysis mode:** `POST /v1/analysis` returns engine evaluation and best moves for a given state. This supports reward shaping, move-quality metrics and "blunder" annotations in replays.
- **FR-O6 Opponent registry:** `GET /v1/games/{id}/opponents` lists the available opponents, their levels and calibrated ratings, including configured LLM players and their measured ratings.

### 7.1 Built-in LLM player

The server can run an LLM as a player, so LLM-vs-engine, LLM-vs-LLM and human-vs-LLM matches, benchmark runs and the info-route eval (FR-I8) need no client code. It is an ordinary opponent type, written `llm:<provider>/<model>`, for example `llm:anthropic/claude-opus-5-5`.

- **FR-L1 Two play modes:**
  - `tools` (default): the model gets the match briefing (§8.5) as its system prompt and plays through the same tools an MCP agent has (`get_state`, `make_move`, plus `simulate_moves` or `analyze_position` when the match allows them). Its transcripts look the same as an external MCP agent's.
  - `direct`: one request per move containing the briefing, the current observation and the legal actions. The model answers with structured output, `{"reasoning": "...", "action": "..."}`, validated against a schema. Cheaper and faster; used for baselines and single-decision benchmark tasks.
- **FR-L2 Configuration:** model, mode, prompt template version, effort level, per-move output-token limit, per-game token and cost caps, move timeout, and retries after an illegal move (default 2, then the match's illegal-move policy applies). Settings that change playing strength (model, mode, effort, prompt version, allowed assists) define a separate agent identity, so each combination gets its own rating.
- **FR-L3 Providers:** behind an `LlmProvider` trait. The first provider is Anthropic's Messages API, called over HTTP from Rust (there is no official Rust SDK), with `claude-opus-5-5` as the default model; any model ID the provider accepts can be configured. Other providers can be added behind the same trait, including an OpenAI-compatible HTTP provider for self-hosted open models.
- **FR-L4 Anthropic provider notes** (API constraints the implementation must follow):
  - Adaptive thinking on models that support it, with `output_config.effort` set explicitly on every request, because default effort differs between models (e.g. `medium` on `claude-opus-5-5`).
  - `thinking.display: "summarized"`, so a readable summary of the model's reasoning can be stored as the move's `reasoning`. The default returns no reasoning text.
  - Game tools are declared with `strict: true`, and the model is steered by instructions with `tool_choice: auto`, because recent models (e.g. `claude-opus-5-5`) reject forced tool choice. `direct` mode uses structured outputs (`output_config.format`) instead of a forced tool call.
  - Tool definitions and the briefing come first and never change during a match, so later turns read them from the prompt cache. Cache hits are tracked from `usage.cache_read_input_tokens`; a briefing shorter than the model's minimum cacheable length isn't cached.
  - The conversation is append-only, because recent models can reject edited history that contains thinking blocks. Long games in `tools` mode use the API's context editing (beta) to clear old tool results instead of trimming history by hand.
  - `refusal` and `max_tokens` stop reasons count as a failed move attempt (retry, then the illegal-move policy), never as a server error.
- **FR-L5 Transcripts and accounting:** every request and response (messages, tool calls, reasoning summaries), token usage (input, output, cache reads and writes), latency, stop reason and estimated cost are stored per move and summed per game. They appear in the agent inspector (FR-F6), benchmark reports (§9) and dataset exports (FR-T5).
- **FR-L6 Keys and budgets:** provider API keys are server-side secrets. They are never stored with match data or returned by the API, and the LLM player is disabled when no key is configured. Spending caps apply per match, per benchmark run and per API key; an LLM seat that hits its cap forfeits with `outcome.reason: "budget_exhausted"`.
- **FR-L7 Reproducibility:** model output isn't deterministic, so benchmark suites repeat tasks (FR-B6) and reports record the full player configuration. Replays stay exact because every action is logged.
- **FR-L8 Batch pricing:** single-decision tasks, which need one response per position, can go through a provider's batch API where one exists. Anthropic's Message Batches API runs asynchronously at 50% of the standard price.

## 8. Functional requirements — Standardized API

### 8.1 Design principles
1. **Same verbs for every game.** Game-specific details live only in the data (schemas, rules, notation), never in endpoints.
2. **Self-describing.** An agent with zero prior knowledge can find out everything it needs from the API itself.
3. **Stateless-friendly.** Every response includes the full current observation and legal actions, so a client never needs to keep its own game state.
4. **Recoverable errors.** Errors are structured, have stable codes and include a `hint` in plain English plus the data needed to retry (e.g. the legal moves).
5. **Idempotent and race-safe.** Moves carry the `turn` number the agent believes is current. Stale or duplicate submissions are rejected cleanly or deduplicated with `Idempotency-Key`.
6. **Versioned.** `/v1/…` for the REST paths; engine versions are recorded on every match.
7. **Assists are explicit.** Anything that helps an agent beyond the rules (engine analysis, simulation) is a per-match setting, recorded with the match and shown with every result.

### 8.2 Core concepts

| Concept | Description |
|---|---|
| `Game` | A type of game (`chess`), with a `GameSpec` |
| `Match` | One play-through of a game with a config, seed, seats and status (`waiting`, `active`, `finished`, `aborted`) |
| `Seat` | A player position in a match (`0`/`white`), filled by a human, agent or built-in opponent |
| `Agent` | A registered identity (API key) that fills seats. It has a rating per game |
| `Event` | Append-only record: `match_created`, `action`, `chance`, `message`, `timeout`, `resign`, `forked_from`, `match_finished` |
| `Observation` | Per-seat view: `text`, `json`, optional `tensor`, plus `legal_actions`, `turn`, `to_act`, `clock` |
| `Position` | A validated game state in the game's notation, used to start matches, simulate and define tasks |
| `Position set` | A named, versioned list of positions (§6.4) |
| `Fork` | A match started from another match's state at a given turn (§8.7) |
| `Assists` | Per-match permissions for help beyond the rules: `allow_analysis`, `allow_simulation` |
| `Benchmark suite` · `Benchmark run` | A versioned, fixed set of tasks with a scoring rule, and one agent's attempt at it (§9) |

### 8.3 REST endpoints (v1)

| Method & path | Purpose |
|---|---|
| `GET /v1/games` | List games with short descriptions and properties |
| `GET /v1/games/{game_id}` | Full `GameSpec`: rules markdown, config/action/observation JSON Schemas, notation, examples, opponents |
| `GET /v1/games/{game_id}/info` | **Model briefing:** everything a model needs to play the game, in one response (§8.4) |
| `POST /v1/games/{game_id}/simulate` | Play move sequences from a position without changing any match (§8.6) |
| `POST /v1/games/{game_id}/positions/validate` | Validate a position and return it in all encodings (FR-E10) |
| `POST /v1/matches` | Create a match: `{game_id, config, seed?, start?, seats: [{type: "self" \| "opponent" \| "agent" \| "human" \| "open", ...}], time_control?, assists?, visibility}` → match, seat tokens and the compact match briefing. `start` is a position (§8.7) |
| `GET /v1/matches/{id}` | Match metadata and status |
| `GET /v1/matches/{id}/info?seat=` | **Match briefing:** the game info plus this match's settings (§8.5) |
| `GET /v1/matches/{id}/state?seat=` | Current observation for a seat (or spectator) |
| `GET /v1/matches/{id}/legal-actions?seat=` | Legal actions in all encodings (string, JSON, index) plus mask |
| `POST /v1/matches/{id}/actions` | Submit `{seat, turn, action, reasoning?}`. `action` can be a string (`"e2e4"`), JSON or `{index: n}` → step result |
| `POST /v1/matches/{id}/resign` · `/offer-draw` | Standard match controls |
| `POST /v1/matches/{id}/fork` | Start a new match from this one at a given turn (§8.7) |
| `GET /v1/matches/{id}/events?since=` | Event log (paginated). The basis for replays |
| `GET /v1/matches/{id}/replay` | Full replay bundle: spec version, config, seed, events, per-step observations (omniscient if permitted) |
| `GET /v1/matches/{id}/llm-calls?seat=` | Built-in LLM player transcript, token usage and cost for a seat (§7.1) |
| `WS /v1/matches/{id}/stream` | Live push of events and observations (players and spectators) |
| `GET /v1/matches?game_id=&agent_id=&status=` | Search history |
| `POST /v1/analysis` | Engine analysis of a state |
| `GET /v1/position-sets?game_id=` · `GET /v1/position-sets/{set_id}` | Position sets and their positions (§6.4) |
| `POST /v1/tournaments` · `GET /v1/tournaments/{id}` | Round-robin / gauntlet evaluation runs |
| `GET /v1/leaderboards/{game_id}` | Ratings by agent |
| `GET /v1/benchmarks` · `GET /v1/benchmarks/{suite_id}` | Benchmark suites and their public tasks (§9) |
| `POST /v1/benchmarks/{suite_id}/runs` | Start a benchmark run for an agent or built-in player (§9) |
| `GET /v1/benchmark-runs/{run_id}` · `/next` · `/report` | Run progress, the next task for a self-driven agent, and the final report |
| `POST /v1/exports` | Export matches (filters) as JSONL/Parquet |
| `POST /v1/batch/step` | RL batch interface: create/reset/step N environments in one call |

An OpenAPI 3.1 spec is generated from the Rust types (`utoipa`) and published at `/v1/openapi.json`, with interactive docs at `/docs`.

### 8.4 Game info route (`GET /v1/games/{game_id}/info`)

Every game has an info route that returns, in one response, **everything a model needs to play that game correctly without any other documentation**. It is the first call an agent makes after choosing a game. `GameSpec` (`GET /v1/games/{game_id}`) is the machine-oriented contract for tooling. The info route is the model-oriented briefing: it is built from the same `GameSpec` plus live server data (opponents, ratings, defaults), so the two can never disagree. For a specific match, the match info route (§8.5) adds that match's settings.

**Query parameters**

| Parameter | Values | Default | Effect |
|---|---|---|---|
| `format` | `json` · `markdown` | `json` | `markdown` returns one document ready to paste into a prompt. `json` returns the same content as structured fields. |
| `detail` | `compact` · `full` | `full` | `compact` drops schemas and long examples (target < 1.5k tokens). `full` includes everything (target < 6k tokens for chess). |
| `config` | JSON (URL-encoded) | game defaults | Tailors the response to a specific configuration, e.g. board size 9 for Go or a chess variant. |
| `seat` | seat id | — | Adds seat-specific notes, e.g. "You play Black and move second". |

**Response contents** (sections are the same for every game and always appear in this order; a section that doesn't apply says so explicitly instead of being omitted):

| Section | Contents | Chess example |
|---|---|---|
| `identity` | `game_id`, name, one-paragraph summary, `engine_version`, `info_version` | `chess`, "Standard chess (FIDE rules)…" |
| `players` | Player count, seat ids and names, who moves first, turn structure | 2 seats: `0` = White (moves first), `1` = Black. Sequential. |
| `properties` | Perfect/imperfect information, stochastic or not, simultaneous moves, max game length, whether the game is solved | Perfect info, deterministic, max 1,000 plies |
| `objective` | How to win, lose and draw, stated plainly | Checkmate wins. Draws: stalemate, threefold repetition, 50-move rule, insufficient material, agreement. |
| `rules` | Complete, concise rules (`rules_markdown`) | Piece movement, castling, en passant, promotion |
| `action_format` | Canonical string notation with grammar and examples, the JSON action schema, discrete index scheme and `action_space_size`, and a list of common mistakes | UCI `<from><to>[promo]`: `e2e4`, `e7e8q`, castling as `e1g1`. Mistake: SAN like `Nf3` is not accepted. |
| `observation_format` | How to read the text board (orientation, coordinates, symbols legend), the JSON observation schema, and tensor shape and plane meanings | Uppercase = White, lowercase = Black, `.` = empty, rank 8 at top. `json.fen` holds FEN. |
| `initial_state` | The starting observation in text and JSON, with its legal actions | Starting position and its 20 legal moves |
| `example_turns` | A short annotated sequence showing the request and response for several moves, including one illegal move and its error | `make_move("e2e4")` → reply `e7e5`; `make_move("e2e5")` → `ILLEGAL_ACTION` with hint |
| `rewards` | Reward range, when rewards are given, default terminal returns, available shaped-reward options | +1 win, 0 draw, −1 loss, only at the end |
| `config_options` | Every config field with type, default, allowed values and effect | `variant: standard \| chess960`, `start_fen?` |
| `time_controls` | Supported time controls, defaults and timeout policy | `none`, per-move, Fischer. Default: none. |
| `opponents` | Available opponents, level ladder with calibrated ratings, and whether analysis and simulation are allowed | `stockfish` 1–10 (≈1350–2850 Elo), `mcts`, `random`, configured `llm:*` players with measured ratings |
| `illegal_move_policy` | What happens after an invalid move and the configurable options | `reject` (default): state unchanged, retry allowed |
| `how_to_play` | The exact API loop for this game with REST paths and MCP tool names | `create_match` → loop `make_move` until `terminated` |
| `strategy_notes` | Optional short, neutral tips on basic principles (off in `detail=compact`; can be disabled per match for evaluations) | Control the centre, develop pieces, king safety |
| `limits` | Rate limits, max tokens of an observation, max match length | — |

**Requirements**
- **FR-I1** Every registered game must serve a complete info response. The conformance kit (FR-E9) fails a game with any empty required section.
- **FR-I2** Examples in `action_format`, `initial_state` and `example_turns` are **generated by running the engine**, not hand-written, so they are always correct for the current `engine_version`.
- **FR-I3** The markdown and JSON formats are rendered from the same data and contain the same information.
- **FR-I4** Responses are deterministic for a given `(game_id, engine_version, config, detail)`. They carry an `ETag` and `info_version` so agents and prompt caches can reuse them.
- **FR-I5** The info route needs no authentication, so an agent can read it before registering.
- **FR-I6** A token estimate (`approx_tokens`) is included so clients can choose between `compact` and `full`.
- **FR-I7** Hidden-information games document what each seat can and cannot see, and never reveal anything about a specific match.
- **FR-I8** An eval check confirms the info is enough: a baseline LLM (the built-in LLM player in `direct` mode, §7.1) given only the `compact` info response must finish 20 games against a random opponent with an illegal-move rate under 2%. This runs in CI for each game (M2 onward).

### 8.5 Match info route (`GET /v1/matches/{id}/info`)

The match info route returns the game briefing (§8.4) for the match's exact config, with a `match` section first that describes this match. It takes the same `format` and `detail` parameters. The seat defaults to the caller's own seat; spectators get the briefing without the seat-specific parts.

| Field | Contents |
|---|---|
| `match_id`, `status`, `turn` | Where the match stands and who is to act |
| `you` | Your seat and what it means, e.g. "You play Black and move second" |
| `participants` | Every seat: name, type (human, agent, engine, LLM), level or rating |
| `start` | Standard start, a custom position (in notation), or the parent match and turn for a fork |
| `time_control` | Time control and current clocks |
| `illegal_move_policy` | The policy in effect and how many invalid attempts remain before it applies |
| `assists` | Whether `analyze_position` and `simulate_moves` are allowed in this match |
| `task` | For benchmark matches: the goal and success rule, e.g. "Find the winning line; the first wrong move ends the task" |
| `recording` | What is stored (moves, reasoning, transcripts) and who can see the match |

- **FR-I9** The match briefing never includes information the seat can't see.
- **FR-I10** Creating a match (REST or MCP) returns the compact match briefing with the first observation, unless `include_info: false`, so an agent needs no extra call before its first move.

### 8.6 Simulation (`POST /v1/games/{game_id}/simulate`)

Simulation plays move sequences from a position and returns what happens, without creating or changing a match. It is for planning agents (an LLM checking a line before committing to it, tree search) and model-based RL.

```json
{
  "from": { "match_id": "m_01J9Z…", "seat": 1 },
  "lines": [["e7e5", "g1f3", "b8c6"], ["c7c5"]],
  "seed": 42,
  "return": "final"
}
```

- `from` is a match (its current state, or an earlier `turn`) or a position (`{"position": "<notation>"}` or `{"state": {...}}`).
- `lines` holds up to 16 lines of up to 32 moves each (configurable limits), with moves for all seats in turn order.
- `return` is `final` (the observation after each line) or `all` (the observation after every move).
- `seed` seeds chance events during the simulation (dice, card draws, tile spawns).

For each line, the response gives the resulting observations in the usual encodings, `terminated` and `returns`, or the first illegal move and its error if the line breaks the rules.

- **FR-S1 No side effects:** simulation never changes a match, is never recorded as a move, and doesn't touch clocks or illegal-move counters. Calls from a match seat are counted on that seat (calls and moves simulated), so reports can show how much an agent relied on it.
- **FR-S2 No leaks:** simulating from a match only uses what the seat can see. Chance events use the caller's seed, never the match's RNG, so simulation can't reveal future dice or cards. In imperfect-information games, hidden parts of the state are sampled to be consistent with the seat's observation (determinization), unless the caller supplies a full state.
- **FR-S3 Assist flag:** `assists.allow_simulation` is on by default, and benchmark suites set it explicitly. When it is off, the `simulate_moves` tool isn't offered to that seat's MCP session or LLM player, and the endpoint returns `ASSIST_NOT_ALLOWED` for the match. It does the same for any position that matches (by hash) one of the caller's active matches where simulation is off.
- **FR-S4 Consistency:** the conformance kit checks that simulating a line gives exactly the same states as playing it in a match.

### 8.7 Starting positions and forks

Matches can start from any valid position instead of the standard start, and any match can be branched at any turn.

- **FR-P4 Custom starts:** `POST /v1/matches` accepts `start` as `{"position": "<notation>"}`, `{"state": {...}}` or `{"position_id": "chess-endgames-basic@1/krk-001"}`. The position is validated (FR-E10), stored with the match, and shown as the starting point in replays.
- **FR-P5 Forks:** `POST /v1/matches/{id}/fork` with `{turn, seats?, visibility?}` creates a new match that starts from the parent's state at `turn`. Its event log begins with a `forked_from` event (parent and turn), and its replay shows the parent's moves up to the fork point, then its own. Forks can be forked again, so a position can grow a tree of variations.
- **FR-P6 Chance after a fork:** a fork gets a new seed for later chance events, unless `keep_rng: true` keeps the parent's RNG state, so two decisions can be compared under the same dice or cards.
- **FR-P7 Permissions:** anyone who can view a finished match can fork it. A match that is still being played can be forked only by its owner, and never while it is part of a benchmark run. A fork of an imperfect-information match starts from what the forking user is allowed to see, with the hidden parts sampled again, unless that user may see the full state.
- **FR-P8 Uses:** curricula (a start position sampled from a position set on every reset, §8.11), puzzles (a position plus an expected line, used by benchmark tasks, §9), and "what if" play from the replay viewer (FR-F4).

### 8.8 Step result (identical for every game)

```json
{
  "match_id": "m_01J9Z…",
  "turn": 13,
  "accepted_action": { "string": "e7e5", "index": 1234, "json": {"from": "e7", "to": "e5"} },
  "opponent_actions": [ { "seat": 0, "string": "g1f3", "info": {"eval_cp": 34, "depth": 18} } ],
  "observation": {
    "text": "  a b c d e f g h\n8 r n b q k b n r\n…\nBlack to move. Move 2.",
    "json": { "fen": "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2", "…": "…" },
    "legal_actions": ["b8c6", "g8f6", "d7d6", "…"],
    "to_act": [1],
    "clock": { "remaining_ms": [298000, 295500] }
  },
  "reward": 0.0,
  "returns": [0.0, 0.0],
  "terminated": false,
  "truncated": false,
  "outcome": null
}
```

When finished: `terminated: true` and `outcome: {"winner": 0, "reason": "checkmate", "returns": [1, -1]}`.

### 8.9 Error model

```json
{
  "error": {
    "code": "ILLEGAL_ACTION",
    "message": "e2e5 is not a legal move: the pawn on e2 can move to e3 or e4.",
    "hint": "Choose one of legal_actions. Moves use UCI notation: <from><to>[promotion], e.g. e2e4, e7e8q.",
    "details": { "legal_actions": ["e2e3", "e2e4", "…"], "turn": 1 }
  }
}
```

Stable codes: `UNKNOWN_GAME`, `INVALID_CONFIG`, `INVALID_POSITION`, `MATCH_NOT_FOUND`, `NOT_YOUR_TURN`, `STALE_TURN`, `ILLEGAL_ACTION`, `UNPARSEABLE_ACTION`, `MATCH_FINISHED`, `ASSIST_NOT_ALLOWED`, `TIMEOUT`, `UNAUTHORIZED`, `RATE_LIMITED`, `ENGINE_UNAVAILABLE`. Invalid moves never change match state and are logged separately (useful for measuring LLM illegal-move rates). Each match has a configurable illegal-move policy: `reject` (default), `forfeit_after_n` or `random_legal_after_n`.

### 8.10 MCP server (`gfa-mcp`)

Built with the official Rust MCP SDK (`rmcp`). It supports **stdio** (local, one command to launch) and **streamable HTTP** (hosted at `/mcp` on the same server).

**Tools** (small set, verb-first names, flat parameters, every result includes readable `text` content plus `structuredContent` that matches a declared `outputSchema`):

| Tool | Parameters | Returns |
|---|---|---|
| `list_games` | — | ids, names, one-line descriptions, player counts |
| `get_game_info` | `game_id`, `detail?` (`compact`/`full`), `config?`, `match_id?` | The game's info briefing (§8.4) as markdown text, with the JSON in `structuredContent`. With `match_id`, the match briefing (§8.5) |
| `list_opponents` | `game_id` | Opponents and levels |
| `create_match` | `game_id`, `opponent` (e.g. `"stockfish"`), `level?`, `play_as?`, `seed?`, `config?`, `start?` (a position) | `match_id`, your seat, the compact match briefing, first observation (opponent may already have moved) |
| `get_state` | `match_id` | Text board, status, legal actions, turn |
| `get_legal_actions` | `match_id` | Legal actions with notation reminder |
| `make_move` | `match_id`, `action`, `reasoning?` | Step result: your move, opponent reply, new board, outcome if finished |
| `simulate_moves` | `match_id`, `lines` (lists of moves), `from_turn?` | Text board and status at the end of each line, or the first illegal move in a line. Only offered when the match allows simulation (§8.6) |
| `resign` | `match_id` | Final outcome |
| `get_match_history` | `game_id?`, `limit?` | Past matches and results for this agent |
| `get_replay` | `match_id` | Move list with annotations |
| `analyze_position` | `match_id`, `turn?` | Engine evaluation (can be disabled per match to prevent cheating in evals) |
| `start_benchmark` | `suite_id`, `split?` | `run_id` and the first task (§9) |
| `next_benchmark_task` | `run_id` | The next task as a new match with its briefing, or the final report when the run is complete |

**Resources:** `gfa://games/{game_id}/info`, `gfa://matches/{match_id}/info`, `gfa://matches/{match_id}/state`, `gfa://matches/{match_id}/replay` (with subscriptions for live state), `gfa://benchmarks/{suite_id}`.
**Prompts:** `play_game(game_id)` gives a ready-made system prompt that embeds the game's `compact` info and explains the game loop.

**LLM-friendliness requirements**
- **FR-M1** Tool descriptions state when to use the tool and include one example call.
- **FR-M2** `make_move` returns the opponent's reply and the new state in one call, so a full game takes about N tool calls, not 3N.
- **FR-M3** Text boards always include coordinates and whose turn it is. Legal-move lists are sorted and use the game's canonical notation.
- **FR-M4** Illegal moves return an `isError` tool result with the hint and legal moves, never a transport error.
- **FR-M5** Optional `reasoning` is stored with the move and shown in the replay viewer.
- **FR-M6** Tool results stay compact (target < 1.5k tokens for a chess state). Verbose fields are opt-in.

### 8.11 Training interfaces

- **FR-T1 Rust crate:** use `gfa-games` directly as a library with zero network overhead, e.g. `Env::<Connect4>::new(seed)`.
- **FR-T2 Python SDK (`gamesforai`, PyO3 + maturin wheels):**
  - `gamesforai.make("connect4")` returns a **Gymnasium**-compatible env (`reset(seed)`, `step(action)` → `obs, reward, terminated, truncated, info` with `info["action_mask"]`).
  - A **PettingZoo** AEC wrapper for multi-agent games.
  - `VectorEnv(game, n)` runs batched stepping in Rust across threads (rayon) and returns NumPy arrays.
  - `reset(seed, options={"position": ...})` starts from a given position, and `options={"position_set": "chess-endgames-basic@1"}` samples a start position from a set on every reset, for curricula.
  - `env.clone()`, `get_state()` and `set_state()` for in-process simulation and tree search, with the same determinism guarantees as the engine.
  - `opponent="stockfish:5"` option to wrap an env as single-agent against a built-in opponent.
  - A remote mode with the same Python API against a server (`gamesforai.connect(url, api_key)`).
- **FR-T3 Self-play & league:** run matches between agent snapshots. Results feed the rating service.
- **FR-T4 Reward configuration:** default sparse terminal rewards (+1/0/−1). Optional per-game shaped rewards (e.g. engine-eval delta) are declared in config and recorded with the match, so datasets stay interpretable.
- **FR-T5 Dataset export:** trajectories as JSONL (LLM fine-tuning: prompt/observation text, action, reasoning, outcome, and full transcripts for built-in LLM players) and Parquet (RL: obs tensors, actions, masks, rewards). Filters: game, agent, rating range, date, outcome.
- **FR-T6 Evaluation harness:** `gfa tournament --game chess --agents a.yaml,b.yaml --opponents stockfish:1..8 --games 100` produces ratings with confidence intervals and a report.

## 9. Functional requirements — Evaluation & benchmarks

A **benchmark suite** is a fixed, versioned set of tasks with a scoring rule, so results from different agents, people and dates can be compared directly. Tournaments (FR-T6) compare agents with each other; suites measure every agent against the same yardstick.

**Suite definition** (`benchmarks/<suite_id>.yaml`, example):

```yaml
id: chess-tactics-v1
game: chess
kind: line_puzzle              # play_game | line_puzzle | single_decision | solve
positions: chess-puzzles-lichess@1   # pinned by content hash when the suite is published
select: { count: 500, stratify_by: rating, seed: 1 }
splits: { public: 250, hidden: 250 }
assists: { allow_analysis: false, allow_simulation: false }
time_control: { per_move_ms: 120000 }
repetitions: 1
scoring: [solve_rate, solve_rate_by_rating_band, estimated_puzzle_rating]
```

**Task kinds**
- `play_game`: full games against a set opponent, from the standard start or from positions in a set, with seats alternated. Scored on results and an estimated rating against the opponent's calibrated levels.
- `line_puzzle`: a position with an expected line. The opponent's replies come from the line, and the first wrong move fails the task. Other moves are accepted when the reference engine or solver confirms they are just as good (e.g. a different mate in one).
- `single_decision`: one move from a position, scored by the reference engine or solver (e.g. "does the move keep the game-theoretic value?").
- `solve`: single-player puzzles such as Sudoku, scored on solve rate, mistakes and moves.

**Initial suites**

| Suite | Game | Kind | Measures | Main score |
|---|---|---|---|---|
| `ttt-perfect-v1` | Tic-Tac-Toe | `play_game` | Rule-following against a perfect player | Non-loss rate over 20 games, both seats |
| `connect4-ladder-v1` | Connect Four | `play_game` | Playing strength | Estimated level against solver levels 1–10 |
| `connect4-positions-v1` | Connect Four | `single_decision` | Tactical accuracy | Share of moves that keep the position's best value |
| `sudoku-graded-v1` | Sudoku | `solve` | Constraint reasoning | Solve rate by difficulty |
| `chess-tactics-v1` | Chess | `line_puzzle` | Tactical calculation | Estimated puzzle rating |
| `chess-ladder-v1` | Chess | `play_game` | Playing strength | Estimated Elo against Stockfish levels 1–10 |

**Requirements**
- **FR-B1 Immutable versions:** a published suite never changes. Any change to tasks, scoring, assists or opponents is a new version (`-v2`). Suites pin their position sets by content hash, and every report carries the suite's content hash plus the engine and opponent versions used.
- **FR-B2 Public and hidden splits:** the public split can be downloaded and used during development. The hidden split stays on the server and is used for official scores. Hidden-split matches are private to the run and excluded from dataset exports, so the tasks can't leak into training data.
- **FR-B3 Running a suite:**
  - *Server-driven:* the harness plays every task with a built-in player (LLM player, engine or remote agent endpoint).
  - *Self-driven:* an external agent asks for the next task (`GET /v1/benchmark-runs/{run_id}/next` or MCP `next_benchmark_task`) and plays it as a normal match through the usual API.
  - *CLI:* `gfa bench run chess-tactics-v1 --player llm:anthropic/claude-opus-5-5`.

  Runs can be paused and resumed, and they play tasks in parallel up to a configurable limit.
- **FR-B4 Reports:** JSON and markdown, with the suite id, version and hash; the full agent configuration (for LLM players: model, mode, effort, prompt version); engine and opponent versions; the assist level and whether the server enforced it; per-task results linked to replays; aggregate scores with 95% bootstrap confidence intervals; the illegal-move rate; and tokens, latency and cost per task when known. Each report has a stable URL and a page in the web app.
- **FR-B5 Leaderboards:** one per suite version, ranked by the main score, split by assist level, counting only hidden-split runs.
- **FR-B6 Repetitions:** suites declare how many times each task runs, which matters for non-deterministic agents such as LLMs. Reports show the spread across repetitions.

## 10. Functional requirements — Persistence

- **FR-D1 Databases:** PostgreSQL in production, SQLite for local and single-user use. Both are accessed through `sqlx` with compile-time-checked queries and embedded migrations.
- **FR-D2 Event sourcing:** the match event log is the source of truth. The current state is cached in memory (and snapshotted periodically) but can always be rebuilt by replaying events from the seed.
- **FR-D3 Schema (initial):**

| Table | Key columns |
|---|---|
| `games` | `id`, `engine_version`, `spec_json` |
| `agents` | `id`, `name`, `kind` (human/llm/rl/engine), `owner`, `api_key_hash`, `metadata` |
| `matches` | `id`, `game_id`, `engine_version`, `config`, `seed`, `start_position`, `parent_match_id`, `fork_turn`, `assists`, `benchmark_run_id`, `status`, `outcome`, `created_at`, `finished_at`, `tags` |
| `seats` | `match_id`, `seat`, `agent_id`, `opponent_spec` (e.g. `stockfish:level=5`), `final_return`, `simulation_calls`, `analysis_calls` |
| `events` | `match_id`, `seq`, `turn`, `seat`, `kind`, `action_string`, `action_index`, `payload`, `reasoning`, `think_ms`, `created_at` |
| `invalid_actions` | `match_id`, `turn`, `seat`, `raw_input`, `error_code`, `created_at` |
| `llm_calls` | `match_id`, `seat`, `turn`, `model`, `request`, `response`, `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `latency_ms`, `stop_reason`, `cost_usd`, `created_at` |
| `ratings` | `agent_id`, `game_id`, `system` (glicko2), `rating`, `rd`, `volatility`, `games_played` |
| `tournaments` | `id`, `config`, `status`, `results` |
| `position_sets` · `positions` | `set_id`, `version`, `game_id`, `content_hash`, `source`, `license` · `set_id`, `position_id`, `notation`, `tags`, `rating`, `expected` |
| `benchmark_suites` · `benchmark_runs` | `suite_id`, `version`, `definition`, `content_hash` · `run_id`, `suite_id`, `agent_id`, `split`, `status`, `report`, `created_at`, `finished_at` |

- **FR-D4 Rating service:** Glicko-2 per game, updated when a match finishes. Engine opponents have fixed anchor ratings from calibration.
- **FR-D5 Retention:** configurable. High-volume training matches can be stored as `summary_only` (outcome and actions, no per-step observations) or not stored at all (`persist: false`) to protect throughput.
- **FR-D6 Replays** are generated from events, never from stored rendered states, so engine bug fixes cannot silently corrupt old replays. Replays of a match played on an older `engine_version` are flagged if that version can no longer be loaded.

## 11. Functional requirements — Frontend (React + Phaser)

**Stack:** React 19 + TypeScript + Vite, Phaser 3 embedded as a React component, TanStack Query for REST, a native WebSocket client for live streams, and TypeScript types generated from the OpenAPI/JSON Schemas.

**Architecture:** React owns the app shell, routing, panels (move list, clocks, reasoning, evaluation graph) and data. Phaser only renders the board and handles input. Each game provides a **scene plugin**:

```ts
interface GameScenePlugin {
  gameId: string;
  scene: typeof Phaser.Scene;            // renders from observation.json
  render(obs: ObservationJson, opts: { perspective: SeatId; highlights?: Move[] }): void;
  onInput(cb: (action: ActionJson) => void): void; // human move input, validated against legal_actions
  animate?(from: ObservationJson, event: GameEvent): Promise<void>;
}
```

Games without a custom scene fall back to a **generic text renderer** that shows `observation.text` with a legal-move picker, so a new game is playable in the UI the day its engine is added.

**Pages**
- **FR-F1 Game library:** catalog cards and a per-game info page rendered from the info route (rules, notation, opponents), with a "copy as prompt" button for the markdown version.
- **FR-F2 Play:** human vs built-in opponent (including LLM players), human vs agent, or hot-seat, from the standard start or a custom position (pasted notation or a pick from a position set). Legal-move highlighting, clocks, resign/draw, and optional engine hints (off by default).
- **FR-F3 Live spectate:** list of active public matches. Live board over WebSocket with the move list, clocks, agent reasoning stream and engine eval bar (if enabled). Multiple matches can be shown in a grid ("arena view") to watch a tournament.
- **FR-F4 Replay viewer:** timeline scrubber, step forward/back, autoplay speed, per-move reasoning and engine annotations (blunder/mistake/inaccuracy), eval graph, perspective toggle (player view vs omniscient for imperfect-information games), and shareable links to a specific move. **Branch from here** forks the match at the current move (FR-P5) so you can play on against any opponent; variations appear as a tree next to the move list.
- **FR-F5 History & leaderboards:** searchable match history (filter by game/agent/result), agent profile pages, rating charts.
- **FR-F6 Agent inspector:** for a match, show each raw request/response pair from the agent (tool calls, invalid attempts, simulations, latency). For built-in LLM players it also shows the full transcript, reasoning summaries, and tokens and cost per move. Useful for debugging LLM agents.
- **FR-F7 Accessibility & responsiveness:** keyboard move entry (notation input), screen-reader text board, and layouts down to 360px width.
- **FR-F8 Benchmarks:** suite pages (description, task kinds, public tasks), live run progress, and report pages with per-task drill-down into replays (public split only) and side-by-side comparison of reports.

## 12. Cross-cutting requirements

- **Auth:** API keys (hashed) for agents, sent as `Authorization: Bearer`. Seat tokens limit an agent to its own seat. Local mode can run with auth disabled. Optional OAuth for human accounts (later phase). LLM provider keys are server-side secrets (FR-L6).
- **Time controls:** per match (`none`, `per_move_ms`, `fischer {base, increment}`). The server enforces timeouts with a configurable policy (forfeit or random legal move) and records think time for every move.
- **Visibility:** matches are `private`, `unlisted` or `public` (appears in live spectate).
- **Rate limiting:** per API key, token-bucket, with separate higher limits for batch endpoints.
- **Observability:** `tracing` with OpenTelemetry export, Prometheus metrics (matches active, steps/sec, engine latency, illegal-move rate, simulation calls, LLM tokens and cost), structured JSON logs, health and readiness endpoints.
- **Deployment:** one Docker image for the server (includes Stockfish and other engine binaries plus the built frontend served statically), `docker-compose.yml` with Postgres, and the existing `docker-publish` workflow publishing to GHCR on version tags. `gfa serve --sqlite ./gfa.db` for zero-dependency local use.
- **Releases:** release-please with Conventional Commits (already configured). Rust crates, the Python wheel and the Docker image are versioned together.
- **Licensing:** the repository is GPL-3.0, which is compatible with distributing Stockfish (GPL-3.0). Every bundled engine's license is checked before inclusion and listed in `THIRD_PARTY_LICENSES`.

## 13. Non-functional requirements

| Area | Target |
|---|---|
| In-process step throughput (single core) | Tic-Tac-Toe ≥ 5M steps/s · Connect Four ≥ 2M steps/s · Chess ≥ 500k steps/s (random playouts incl. legal-move generation) |
| Vectorized env (Python, 8 cores, Connect Four) | ≥ 1M steps/s including NumPy transfer |
| REST `POST /actions` latency (excluding opponent think time) | p50 < 5 ms, p99 < 25 ms |
| Simulation (`POST /simulate`, 16 lines × 32 moves, chess) | p99 < 20 ms |
| WebSocket event fan-out to spectators | < 100 ms p95 |
| Concurrent active matches per server instance (4 vCPU) | ≥ 10,000 lightweight matches; engine-backed matches limited by the engine pool |
| Determinism | 100% of conformance-suite replays reproduce identical states |
| Availability (hosted) | 99.5% monthly for v1 |
| Test coverage | Conformance kit for every game; ≥ 80% line coverage in `gfa-core` and `gfa-server` |
| Security | Engine processes sandboxed; no user-controlled strings reach engine stdin without validation; API keys hashed with argon2 |

## 14. Success metrics

- **Time-to-first-game for an LLM agent:** under 5 minutes from `docker compose up` to Claude finishing a chess game against Stockfish through MCP.
- **Game onboarding cost:** a new perfect-information game (engine plus conformance tests plus text rendering) in ≤ 1 day, with zero changes to server, MCP or frontend core.
- **API uniformity:** 0 game-specific endpoints or MCP tools.
- **Adoption:** the Python SDK is used in at least one external training run. Leaderboards exist for ≥ 3 games.
- **Data quality:** 100% of finished matches can be replayed. Exported datasets load in Hugging Face `datasets` without custom code.
- **Comparable results:** the six initial benchmark suites have published reports for at least three different agents, and running a deterministic agent (an engine) twice on a suite gives identical reports.

## 15. Milestones

| Milestone | Scope | Exit criteria |
|---|---|---|
| **M0 – Foundations** | Cargo workspace, `gfa-core` trait + `DynGame`, conformance kit, CI (fmt, clippy, test), Tic-Tac-Toe | Tic-Tac-Toe passes conformance; benchmarks in CI |
| **M1 – Playable API** | `gfa-server` REST + WS, game and match info routes, SQLite/Postgres store, event log, Connect Four, Sudoku, built-in minimax/MCTS/random opponents, position import/export, custom starts and forks, simulation, OpenAPI | Full match lifecycle via curl for two-player and single-player games; replays rebuild exactly, including forks; simulation matches real play move for move |
| **M2 – MCP, Chess + LLM player** | `gfa-mcp` (stdio + HTTP) including `simulate_moves`, chess engine (shakmaty), UCI adapter + Stockfish, difficulty ladder, built-in LLM player (Anthropic provider, `tools` and `direct` modes) | Claude completes chess games vs Stockfish levels 1–10, both through MCP and as the built-in LLM player; illegal moves are recoverable; the FR-I8 eval runs for every MVP game |
| **M3 – Frontend** | React+Phaser shell, library, play (incl. custom positions and LLM opponents), live spectate, replay viewer with branch-from-here, generic text renderer, chess/C4/TTT/Sudoku scenes | Human can play and replay all MVP games and branch from any move; live AI-vs-AI viewable |
| **M4 – Training** | PyO3 SDK, Gymnasium/PettingZoo wrappers, VectorEnv, position sets and curricula, `clone`/`get_state`/`set_state`, batch REST, exports, ratings, tournaments CLI | Throughput targets met; PPO example trains Connect Four agent that beats level 3 |
| **M5 – Evaluation** | Benchmark suite format and runner (server-driven, self-driven, CLI, MCP), puzzle position sets, the six initial suites with public and hidden splits, reports, per-suite leaderboards, benchmark pages in the web app | Every initial suite runs end to end; reports published for at least three agents; an engine run twice gives identical reports |
| **M6 – Catalog expansion** | Checkers, Othello, Go + KataGo (GTP), 2048 | Each passes conformance and has a calibrated opponent ladder |
| **M7 – Imperfect information** | Hold'em, Battleship, Kuhn/Liar's Dice, Hanabi; omniscient replay view | Hidden-info leakage tests pass; CFR baselines available |

## 16. Future phases

Planned after M7, in no fixed order:

- **F1 – Human game import and demonstrations.** Import PGN (chess), SGF (Go) and JSONL game records. Every move is checked by the engine, and each game is stored as a match with `source: imported`, its provenance and its license (e.g. the Lichess game database, CC0). Games humans play in the web app can be opted in as demonstrations. Exports can filter by source, so imitation-learning data is available as soon as a game ships.
- **F2 – Move-quality and cost metrics.** An analysis worker scores every finished match: evaluation loss per move (centipawn loss in chess, win-probability loss elsewhere), blunder, mistake and inaccuracy counts, illegal-move rate, and tokens, latency and cost per move for LLM players. Shown on agent profiles, in benchmark reports and in dashboards. Builds on FR-O5 and FR-L5.
- **F3 – Move validation.** `POST /v1/games/{game_id}/validate-action` (MCP: `check_move`) checks a move's notation and legality in a position or a match's current state and returns the parsed forms and a hint, without playing it. It reveals nothing beyond the legal-move list, so it stays available when assists are off.
- **F4 – Info changelog.** `GET /v1/games/{game_id}/info/changes?since=<info_version>` lists what changed in a briefing (rules, notation, config options, opponents), so agents and prompt caches can refresh a cached copy. `info_version` gets a major bump for rules or notation changes.
- **F5 – Communication games.** A message channel between seats (`POST /v1/matches/{id}/messages`) with public and private channels and per-game message rules (free text for negotiation, fixed vocabularies where the rules require them). Messages are events with per-seat visibility and appear in replays. Target games: Diplomacy-style negotiation and Codenames-style word games, to measure cooperation, persuasion and deception detection.
- **F6 – Real-time games.** A tick-based `RealtimeGame` interface: a fixed tick rate, simultaneous actions every tick, and a default action for a player who doesn't answer in time. Live play runs over WebSocket; in-process training runs as fast as the CPU allows. Target games: Snake, Pong and Tron-style light cycles.

## 17. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Engine processes (Stockfish/KataGo) are heavy under load | Pool with limits, queue with back-pressure, cheaper built-in opponents for training, GPU-optional KataGo config |
| Leaking hidden info through observations or MCP tools | Viewer-scoped `observe`, property tests, `analyze_position` disabled for imperfect-information and eval matches |
| LLM agents cheat by calling analysis tools | Per-match `assists` flags (`allow_analysis`, `allow_simulation`) recorded with the match; leaderboards split results by assist level |
| Game-trait design turns out too narrow (simultaneous, stochastic, n-player) | Model these from M0 (`current_players` vec, chance events, `Viewer`), validate against OpenSpiel's game taxonomy |
| DB write volume from RL training | `persist` levels, batched inserts, optional off-by-default storage for batch endpoints |
| Engine version changes invalidate old replays | `engine_version` stored per match; old engine versions stay loadable behind feature flags or replays are flagged |
| License conflicts with bundled engines | License review gate per engine; GPL-3.0 repo license |
| Agents use their own engines or simulators when assists are off | The server can only enforce assists for tools it provides (MCP, LLM player), so reports and leaderboards label each assist level as server-enforced or self-reported |
| Benchmark tasks leak into training data | Hidden splits stay on the server and are excluded from exports; new suite versions rotate tasks |
| LLM provider costs or rate limits stall benchmark runs | Spending caps per run, concurrency limits, resumable runs, `direct` mode, and batch APIs for single-decision tasks |
| Simulation or forks reveal hidden information or future chance events | Seat-scoped simulation with sampled hidden state and a caller-seeded RNG (FR-S2); fork permissions (FR-P7) |

## 18. Open questions

1. Hosted multi-tenant service, or self-hosted only for v1?
2. Should LLM agents be able to register as **remote players** (the server calls them through a webhook) in addition to calling the API themselves?
3. Should Chess960 and other variants be separate `game_id`s or config options of `chess`? (Proposed: config options, with `variant` in the spec.)
4. Is Parquet export needed in M4, or is JSONL enough for the first training users?
5. Real-time games (F6): a fixed tick rate per game or one set per match, and how should network latency be handled for remote agents?
6. Should human accounts and ratings share the leaderboards with agents?
7. Hidden benchmark splits only work on a server the agent's owner doesn't control. Will there be an official hosted benchmark server, or do self-hosted installs get public splits only? (Related to question 1.)
8. Who pays for built-in LLM player calls on a shared server: each user's own provider key, or a server key with quotas?

## 19. Glossary

- **Seat:** a player position in a match.
- **Observation:** what one seat is allowed to see at a moment in time.
- **Action mask:** boolean vector over the fixed action space marking legal actions.
- **UCI / GTP:** standard text protocols for chess engines and Go engines.
- **MCP:** Model Context Protocol, the standard for exposing tools and resources to LLM agents.
- **Conformance kit:** shared test suite every game must pass.
- **Position set:** a named, versioned list of positions for one game, used for curricula, puzzles and benchmarks.
- **Fork:** a new match that starts from another match's state at a chosen turn.
- **Assists:** per-match permissions for help beyond the rules, such as engine analysis or simulation.
- **Benchmark suite:** a fixed, versioned set of tasks with a scoring rule. Its **hidden split** stays on the server and is used for official scores.
- **Determinization:** sampling the hidden parts of a state so they are consistent with what a player has seen.

## 20. Revision history

| Version | Date | Changes |
|---|---|---|
| v0.1 | 2026-09-29 | Initial draft, game info route (§8.4), Sudoku (§6.3) |
| v0.2 | 2026-09-30 | Position sets (§6.4), built-in LLM player (§7.1), match info route (§8.5), simulation (§8.6), custom starts and forks (§8.7), evaluation and benchmark suites (§9), evaluation milestone (M5), future phases (§16) |
