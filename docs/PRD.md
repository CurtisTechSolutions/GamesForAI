# GamesForAI — Product Requirements Document

| | |
|---|---|
| **Status** | Draft v0.1 |
| **Last updated** | 2026-09-29 |
| **License** | GPL-3.0 (repo license; compatible with bundling Stockfish) |
| **Stack** | Rust backend (engines, API, MCP, storage) · React + Phaser frontend · PostgreSQL / SQLite |

---

## 1. Summary

GamesForAI is a game library built as a **training and evaluation environment for AI systems**. It provides:

1. **Game engines** in Rust. Each game is a separate, deterministic crate behind one shared interface.
2. **One standardized API** exposed through three transports: REST/WebSocket for services, **MCP** for LLM agents, and in-process bindings (Rust and Python) for high-throughput reinforcement learning.
3. **Built-in opponents** for when no human is playing: strong external solvers such as Stockfish for chess, plus generic fallbacks (MCTS, minimax, random).
4. **Persistent game history** in a database. Every match is an append-only event log that can be replayed, analyzed and exported as training data.
5. **A single React + Phaser web app** to browse games, play them, watch live matches and scrub through replays.

The main design principle: **learn the API once, play any game.** An agent that can play tic-tac-toe through the API can play chess through the same calls, reading the rules, observations and legal actions that each game describes about itself.

## 2. Problem & motivation

- Existing RL environments (Gymnasium, PettingZoo, OpenSpiel) are Python- or C++-centric. Each has its own conventions, and none targets **LLM agents**, which need text observations, self-describing rules, readable error messages and tool-calling interfaces.
- LLM agent evaluations on games are usually one-off scripts. Results are not reproducible or comparable, and they are rarely stored in a form that can be reused as training data.
- Setting up a strong opponent (Stockfish, KataGo, a Connect Four solver) for every game is repetitive integration work.
- Researchers want to *see* what their agent is doing: watch it live, replay its games and inspect its reasoning.

## 3. Goals & non-goals

### Goals
- **G1 – Standardized interface:** every game implements the same trait and is reachable through the same endpoints and MCP tools, with no game-specific client code.
- **G2 – LLM/MCP friendly:** self-describing games (rules text, JSON Schemas, examples), text and JSON observations, legal-action listings, and error messages that tell the agent how to recover.
- **G3 – Training friendly:** deterministic seeded engines, fast headless stepping, vectorized batch environments, self-play, and Gymnasium/PettingZoo-compatible wrappers.
- **G4 – Strong baselines:** pluggable opponents at configurable strength for every game.
- **G5 – Full history:** every match is persisted, reproducible from its seed and actions, and exportable (JSONL/Parquet).
- **G6 – Visual tooling:** play, spectate live and replay any match in the browser.

### Non-goals (v1)
- Hosting model training or inference. GamesForAI is the *environment*, not the trainer.
- Real-money play, public matchmaking at internet scale, or anti-cheat for human competitive play.
- Video/3D games or pixel-observation Atari-style emulation. v1 targets turn-based board, card and puzzle games; real-time games are a later phase.
- A mobile-native app. The web app should be responsive, which is sufficient.

## 4. Users & primary use cases

| Persona | Needs | Primary interface |
|---|---|---|
| **LLM agent developer** | Connect Claude or another LLM to games through MCP, evaluate reasoning, collect transcripts | MCP server |
| **RL researcher** | Millions of steps per hour, self-play, deterministic replays, gym-style API | Rust crate / Python SDK (in-process), batch REST |
| **Evaluation engineer** | Run agent-vs-baseline tournaments, get Elo ratings and reproducible reports | REST API + CLI |
| **Human player / observer** | Play against AI, watch AI-vs-AI live, review replays | Web app |
| **Game contributor** | Add a new game without touching the server or frontend core | Rust `Game` trait + Phaser scene plugin |

**Key user stories**
- As an LLM agent, I can call `list_games`, then `get_rules("chess")`, then `create_match` against Stockfish at level 5, and play to completion using only `get_state` and `make_move`.
- As an RL researcher, I can `pip install gamesforai`, create 1,024 parallel Connect Four environments in-process and train with any Gymnasium-compatible library.
- As an evaluator, I can run a 200-game round robin between three agent endpoints and two baselines, then view a leaderboard and download every game as JSONL.
- As a human, I can open a live match between two agents, watch the moves animate, see each agent's optional stated reasoning, and later scrub the replay move by move.

## 5. System overview

```
                    ┌──────────────────────────── web/ (React + Phaser) ───────────────────────────┐
                    │  Lobby · Play · Live spectate · Replay viewer · Leaderboards · Agent inspector │
                    └───────────────▲──────────────────────────────▲───────────────────────────────┘
                                    │ REST                          │ WebSocket (live events)
┌── LLM agents ──┐   MCP (stdio /   │                               │
│ Claude, etc.   │── streamable ───►┌─────────────────── gfa-server (Rust, axum) ───────────────────┐
└────────────────┘   HTTP)          │  API v1 · MCP · Auth · Match orchestrator · Rating service    │
┌── RL trainers ─┐   REST/batch     │                                                               │
│ Python/Rust    │─────────────────►│   ┌──────────── Game registry (dyn Game) ─────────────┐       │
└────────────────┘                  │   │ tictactoe · connect4 · chess · checkers · go · ... │       │
        │ in-process (PyO3 / crate) │   └────────────────────────────────────────────────────┘       │
        └──────────────────────────►│   ┌──────────── Opponent adapters ────────────────────┐       │
                                    │   │ Stockfish (UCI) · KataGo (GTP) · solvers · MCTS   │       │
                                    │   └────────────────────────────────────────────────────┘       │
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
│   │   ├── chess/             # wraps `shakmaty` for move generation
│   │   └── ...
│   ├── gfa-opponents/         # Opponent trait + adapters (uci, gtp, mcts, minimax, random)
│   ├── gfa-store/             # sqlx repositories, migrations, export (JSONL/Parquet)
│   ├── gfa-server/            # axum HTTP + WebSocket API, match orchestration, auth
│   ├── gfa-mcp/               # MCP server (rmcp), maps tools onto gfa-core/gfa-server
│   ├── gfa-py/                # PyO3 bindings + Gymnasium/PettingZoo wrappers (maturin)
│   └── gfa-cli/               # `gfa` CLI: run server, play, tournaments, export
├── web/                       # React + Vite + Phaser app
│   └── src/games/<game_id>/   # one Phaser scene per game
├── schemas/                   # generated JSON Schemas + OpenAPI spec (source of truth for TS types)
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
- **FR-E6 Rules as data:** `GameSpec.rules_markdown` holds complete, concise rules, win conditions, notation explanation and 1–3 worked examples. This is the text LLM agents receive from `get_rules`.
- **FR-E7 Termination:** games declare a max length. Draw rules (threefold repetition, 50-move rule, etc.) are enforced by the engine. Truncation is distinct from termination in the step result.
- **FR-E8 Performance:** engines are allocation-light. Targets are listed in §12.
- **FR-E9 Conformance test kit:** `gfa-core::testing::conformance::<G>()` runs random playouts to check determinism, serialization round-trips, legal-action/apply consistency, index/string encoding bijections, observation schema validity and hidden-info non-leakage. Every game must pass it in CI.

### 6.2 Game catalog

| Phase | Game | Players | Info | Why it's included | Reference opponent |
|---|---|---|---|---|---|
| MVP | Tic-Tac-Toe | 2 | Perfect | Smoke test, trivially solvable | Perfect minimax |
| MVP | Connect Four | 2 | Perfect | Solved game, graded difficulty | Bitboard negamax solver (Pons-style), depth-limited for levels |
| MVP | Chess | 2 | Perfect | Flagship, strong engine available | **Stockfish** (UCI), Skill Level 0–20 / UCI_Elo |
| P2 | Checkers (English draughts) | 2 | Perfect | Classic, solved | Alpha-beta engine |
| P2 | Othello | 2 | Perfect | Medium complexity | Edax (GPL) or built-in alpha-beta |
| P2 | Go (9×9, 13×13, 19×19) | 2 | Perfect | Deep planning | **KataGo** (GTP), GNU Go fallback |
| P2 | 2048 / Sokoban | 1 | Perfect, stochastic / deterministic | Single-agent planning | Expectimax / solver |
| P3 | Heads-up Limit Texas Hold'em | 2 | Imperfect, stochastic | Bluffing, hidden info | CFR-based bot |
| P3 | Battleship | 2 | Imperfect | Inference under uncertainty | Probability-density heuristic |
| P3 | Liar's Dice / Kuhn Poker | 2+ | Imperfect | Small game-theory testbeds | Exact Nash / CFR |
| P3 | Hanabi | 2–5 | Imperfect, cooperative | Cooperation, theory of mind | Rule-based bots |

## 7. Functional requirements — Opponents (built-in players)

- **FR-O1 `Opponent` trait:** `choose_action(observation, legal_actions, clock) -> ActionChoice { action, info }`, where `info` can include eval score, principal variation, search depth and nodes. Opponents only see what a player is allowed to see.
- **FR-O2 Adapters:**
  - `uci`: generic UCI engine process manager, used for Stockfish (and Lc0 later). Supports `Skill Level`, `UCI_LimitStrength`/`UCI_Elo`, movetime/nodes/depth limits and `MultiPV` for analysis.
  - `gtp`: generic GTP adapter for Go engines (KataGo, GNU Go).
  - `builtin`: in-process Rust solvers (minimax/alpha-beta, MCTS, expectimax, CFR). Every game gets MCTS and `random` for free through the `Game` trait.
  - `remote_agent`: an external agent reachable via HTTP callback or WebSocket. This is how two user agents play each other through the server.
- **FR-O3 Standard difficulty ladder:** each opponent exposes `level: 1..=10`, mapped per engine (e.g. Stockfish UCI_Elo 1350→2850; Connect Four search depth). Levels are calibrated so that level *n+1* beats level *n* about 70% of the time. Calibration runs as a scheduled job and results are published in the game spec.
- **FR-O4 Engine process management:** engine processes are pooled and reused. Each has hard CPU-time, memory and wall-clock limits, is restarted when it crashes, and runs sandboxed (no network, read-only FS). Engine binaries ship in the Docker image and their paths are configurable.
- **FR-O5 Analysis mode:** `POST /v1/analysis` returns engine evaluation and best moves for a given state. This supports reward shaping, move-quality metrics and "blunder" annotations in replays.
- **FR-O6 Opponent registry:** `GET /v1/games/{id}/opponents` lists the available opponents, their levels and calibrated ratings.

## 8. Functional requirements — Standardized API

### 8.1 Design principles
1. **Same verbs for every game.** Game-specific details live only in the data (schemas, rules, notation), never in endpoints.
2. **Self-describing.** An agent with zero prior knowledge can find out everything it needs from the API itself.
3. **Stateless-friendly.** Every response includes the full current observation and legal actions, so a client never needs to keep its own game state.
4. **Recoverable errors.** Errors are structured, have stable codes and include a `hint` in plain English plus the data needed to retry (e.g. the legal moves).
5. **Idempotent and race-safe.** Moves carry the `turn` number the agent believes is current. Stale or duplicate submissions are rejected cleanly or deduplicated with `Idempotency-Key`.
6. **Versioned.** `/v1/…` for the REST paths; engine versions are recorded on every match.

### 8.2 Core concepts

| Concept | Description |
|---|---|
| `Game` | A type of game (`chess`), with a `GameSpec` |
| `Match` | One play-through of a game with a config, seed, seats and status (`waiting`, `active`, `finished`, `aborted`) |
| `Seat` | A player position in a match (`0`/`white`), filled by a human, agent or built-in opponent |
| `Agent` | A registered identity (API key) that fills seats. It has a rating per game |
| `Event` | Append-only record: `match_created`, `action`, `chance`, `message`, `timeout`, `resign`, `match_finished` |
| `Observation` | Per-seat view: `text`, `json`, optional `tensor`, plus `legal_actions`, `turn`, `to_act`, `clock` |

### 8.3 REST endpoints (v1)

| Method & path | Purpose |
|---|---|
| `GET /v1/games` | List games with short descriptions and properties |
| `GET /v1/games/{game_id}` | Full `GameSpec`: rules markdown, config/action/observation JSON Schemas, notation, examples, opponents |
| `POST /v1/matches` | Create a match: `{game_id, config, seed?, seats: [{type: "self"|"opponent"|"agent"|"human"|"open", ...}], time_control?, visibility}` → match plus seat tokens |
| `GET /v1/matches/{id}` | Match metadata and status |
| `GET /v1/matches/{id}/state?seat=` | Current observation for a seat (or spectator) |
| `GET /v1/matches/{id}/legal-actions?seat=` | Legal actions in all encodings (string, JSON, index) plus mask |
| `POST /v1/matches/{id}/actions` | Submit `{seat, turn, action, reasoning?}`. `action` can be a string (`"e2e4"`), JSON or `{index: n}` → step result |
| `POST /v1/matches/{id}/resign` · `/offer-draw` | Standard match controls |
| `GET /v1/matches/{id}/events?since=` | Event log (paginated). The basis for replays |
| `GET /v1/matches/{id}/replay` | Full replay bundle: spec version, config, seed, events, per-step observations (omniscient if permitted) |
| `WS /v1/matches/{id}/stream` | Live push of events and observations (players and spectators) |
| `GET /v1/matches?game_id=&agent_id=&status=` | Search history |
| `POST /v1/analysis` | Engine analysis of a state |
| `POST /v1/tournaments` · `GET /v1/tournaments/{id}` | Round-robin / gauntlet evaluation runs |
| `GET /v1/leaderboards/{game_id}` | Ratings by agent |
| `POST /v1/exports` | Export matches (filters) as JSONL/Parquet |
| `POST /v1/batch/step` | RL batch interface: create/reset/step N environments in one call |

An OpenAPI 3.1 spec is generated from the Rust types (`utoipa`) and published at `/v1/openapi.json`, with interactive docs at `/docs`.

### 8.4 Step result (identical for every game)

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

### 8.5 Error model

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

Stable codes: `UNKNOWN_GAME`, `INVALID_CONFIG`, `MATCH_NOT_FOUND`, `NOT_YOUR_TURN`, `STALE_TURN`, `ILLEGAL_ACTION`, `UNPARSEABLE_ACTION`, `MATCH_FINISHED`, `TIMEOUT`, `UNAUTHORIZED`, `RATE_LIMITED`, `ENGINE_UNAVAILABLE`. Invalid moves never change match state and are logged separately (useful for measuring LLM illegal-move rates). Each match has a configurable illegal-move policy: `reject` (default), `forfeit_after_n` or `random_legal_after_n`.

### 8.6 MCP server (`gfa-mcp`)

Built with the official Rust MCP SDK (`rmcp`). It supports **stdio** (local, one command to launch) and **streamable HTTP** (hosted at `/mcp` on the same server).

**Tools** (small set, verb-first names, flat parameters, every result includes readable `text` content plus `structuredContent` that matches a declared `outputSchema`):

| Tool | Parameters | Returns |
|---|---|---|
| `list_games` | — | ids, names, one-line descriptions, player counts |
| `get_rules` | `game_id` | Rules markdown, notation guide, examples |
| `list_opponents` | `game_id` | Opponents and levels |
| `create_match` | `game_id`, `opponent` (e.g. `"stockfish"`), `level?`, `play_as?`, `seed?`, `config?` | `match_id`, your seat, first observation (opponent may already have moved) |
| `get_state` | `match_id` | Text board, status, legal actions, turn |
| `get_legal_actions` | `match_id` | Legal actions with notation reminder |
| `make_move` | `match_id`, `action`, `reasoning?` | Step result: your move, opponent reply, new board, outcome if finished |
| `resign` | `match_id` | Final outcome |
| `get_match_history` | `game_id?`, `limit?` | Past matches and results for this agent |
| `get_replay` | `match_id` | Move list with annotations |
| `analyze_position` | `match_id`, `turn?` | Engine evaluation (can be disabled per match to prevent cheating in evals) |

**Resources:** `gfa://games/{game_id}/rules`, `gfa://matches/{match_id}/state`, `gfa://matches/{match_id}/replay` (with subscriptions for live state).
**Prompts:** `play_game(game_id)` gives a ready-made system prompt that explains the game loop.

**LLM-friendliness requirements**
- **FR-M1** Tool descriptions state when to use the tool and include one example call.
- **FR-M2** `make_move` returns the opponent's reply and the new state in one call, so a full game takes about N tool calls, not 3N.
- **FR-M3** Text boards always include coordinates and whose turn it is. Legal-move lists are sorted and use the game's canonical notation.
- **FR-M4** Illegal moves return an `isError` tool result with the hint and legal moves, never a transport error.
- **FR-M5** Optional `reasoning` is stored with the move and shown in the replay viewer.
- **FR-M6** Tool results stay compact (target < 1.5k tokens for a chess state). Verbose fields are opt-in.

### 8.7 Training interfaces

- **FR-T1 Rust crate:** use `gfa-games` directly as a library with zero network overhead, e.g. `Env::<Connect4>::new(seed)`.
- **FR-T2 Python SDK (`gamesforai`, PyO3 + maturin wheels):**
  - `gamesforai.make("connect4")` returns a **Gymnasium**-compatible env (`reset(seed)`, `step(action)` → `obs, reward, terminated, truncated, info` with `info["action_mask"]`).
  - A **PettingZoo** AEC wrapper for multi-agent games.
  - `VectorEnv(game, n)` runs batched stepping in Rust across threads (rayon) and returns NumPy arrays.
  - `opponent="stockfish:5"` option to wrap an env as single-agent against a built-in opponent.
  - A remote mode with the same Python API against a server (`gamesforai.connect(url, api_key)`).
- **FR-T3 Self-play & league:** run matches between agent snapshots. Results feed the rating service.
- **FR-T4 Reward configuration:** default sparse terminal rewards (+1/0/−1). Optional per-game shaped rewards (e.g. engine-eval delta) are declared in config and recorded with the match, so datasets stay interpretable.
- **FR-T5 Dataset export:** trajectories as JSONL (LLM fine-tuning: prompt/observation text, action, reasoning, outcome) and Parquet (RL: obs tensors, actions, masks, rewards). Filters: game, agent, rating range, date, outcome.
- **FR-T6 Evaluation harness:** `gfa tournament --game chess --agents a.yaml,b.yaml --opponents stockfish:1..8 --games 100` produces ratings with confidence intervals and a report.

## 9. Functional requirements — Persistence

- **FR-D1 Databases:** PostgreSQL in production, SQLite for local and single-user use. Both are accessed through `sqlx` with compile-time-checked queries and embedded migrations.
- **FR-D2 Event sourcing:** the match event log is the source of truth. The current state is cached in memory (and snapshotted periodically) but can always be rebuilt by replaying events from the seed.
- **FR-D3 Schema (initial):**

| Table | Key columns |
|---|---|
| `games` | `id`, `engine_version`, `spec_json` |
| `agents` | `id`, `name`, `kind` (human/llm/rl/engine), `owner`, `api_key_hash`, `metadata` |
| `matches` | `id`, `game_id`, `engine_version`, `config`, `seed`, `status`, `outcome`, `created_at`, `finished_at`, `tags` |
| `seats` | `match_id`, `seat`, `agent_id`, `opponent_spec` (e.g. `stockfish:level=5`), `final_return` |
| `events` | `match_id`, `seq`, `turn`, `seat`, `kind`, `action_string`, `action_index`, `payload`, `reasoning`, `think_ms`, `created_at` |
| `invalid_actions` | `match_id`, `turn`, `seat`, `raw_input`, `error_code`, `created_at` |
| `ratings` | `agent_id`, `game_id`, `system` (glicko2), `rating`, `rd`, `volatility`, `games_played` |
| `tournaments` | `id`, `config`, `status`, `results` |

- **FR-D4 Rating service:** Glicko-2 per game, updated when a match finishes. Engine opponents have fixed anchor ratings from calibration.
- **FR-D5 Retention:** configurable. High-volume training matches can be stored as `summary_only` (outcome and actions, no per-step observations) or not stored at all (`persist: false`) to protect throughput.
- **FR-D6 Replays** are generated from events, never from stored rendered states, so engine bug fixes cannot silently corrupt old replays. Replays of a match played on an older `engine_version` are flagged if that version can no longer be loaded.

## 10. Functional requirements — Frontend (React + Phaser)

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
- **FR-F1 Game library:** catalog cards, rules viewer and opponent list per game.
- **FR-F2 Play:** human vs built-in opponent, human vs agent, or hot-seat. Legal-move highlighting, clocks, resign/draw, and optional engine hints (off by default).
- **FR-F3 Live spectate:** list of active public matches. Live board over WebSocket with the move list, clocks, agent reasoning stream and engine eval bar (if enabled). Multiple matches can be shown in a grid ("arena view") to watch a tournament.
- **FR-F4 Replay viewer:** timeline scrubber, step forward/back, autoplay speed, per-move reasoning and engine annotations (blunder/mistake/inaccuracy), eval graph, perspective toggle (player view vs omniscient for imperfect-information games), and shareable links to a specific move.
- **FR-F5 History & leaderboards:** searchable match history (filter by game/agent/result), agent profile pages, rating charts.
- **FR-F6 Agent inspector:** for a match, show each raw request/response pair from the agent (tool calls, invalid attempts, latency). Useful for debugging LLM agents.
- **FR-F7 Accessibility & responsiveness:** keyboard move entry (notation input), screen-reader text board, and layouts down to 360px width.

## 11. Cross-cutting requirements

- **Auth:** API keys (hashed) for agents, sent as `Authorization: Bearer`. Seat tokens limit an agent to its own seat. Local mode can run with auth disabled. Optional OAuth for human accounts (later phase).
- **Time controls:** per match (`none`, `per_move_ms`, `fischer {base, increment}`). The server enforces timeouts with a configurable policy (forfeit or random legal move) and records think time for every move.
- **Visibility:** matches are `private`, `unlisted` or `public` (appears in live spectate).
- **Rate limiting:** per API key, token-bucket, with separate higher limits for batch endpoints.
- **Observability:** `tracing` with OpenTelemetry export, Prometheus metrics (matches active, steps/sec, engine latency, illegal-move rate), structured JSON logs, health and readiness endpoints.
- **Deployment:** one Docker image for the server (includes Stockfish and other engine binaries plus the built frontend served statically), `docker-compose.yml` with Postgres, and the existing `docker-publish` workflow publishing to GHCR on version tags. `gfa serve --sqlite ./gfa.db` for zero-dependency local use.
- **Releases:** release-please with Conventional Commits (already configured). Rust crates, the Python wheel and the Docker image are versioned together.
- **Licensing:** the repository is GPL-3.0, which is compatible with distributing Stockfish (GPL-3.0). Every bundled engine's license is checked before inclusion and listed in `THIRD_PARTY_LICENSES`.

## 12. Non-functional requirements

| Area | Target |
|---|---|
| In-process step throughput (single core) | Tic-Tac-Toe ≥ 5M steps/s · Connect Four ≥ 2M steps/s · Chess ≥ 500k steps/s (random playouts incl. legal-move generation) |
| Vectorized env (Python, 8 cores, Connect Four) | ≥ 1M steps/s including NumPy transfer |
| REST `POST /actions` latency (excluding opponent think time) | p50 < 5 ms, p99 < 25 ms |
| WebSocket event fan-out to spectators | < 100 ms p95 |
| Concurrent active matches per server instance (4 vCPU) | ≥ 10,000 lightweight matches; engine-backed matches limited by the engine pool |
| Determinism | 100% of conformance-suite replays reproduce identical states |
| Availability (hosted) | 99.5% monthly for v1 |
| Test coverage | Conformance kit for every game; ≥ 80% line coverage in `gfa-core` and `gfa-server` |
| Security | Engine processes sandboxed; no user-controlled strings reach engine stdin without validation; API keys hashed with argon2 |

## 13. Success metrics

- **Time-to-first-game for an LLM agent:** under 5 minutes from `docker compose up` to Claude finishing a chess game against Stockfish through MCP.
- **Game onboarding cost:** a new perfect-information game (engine plus conformance tests plus text rendering) in ≤ 1 day, with zero changes to server, MCP or frontend core.
- **API uniformity:** 0 game-specific endpoints or MCP tools.
- **Adoption:** the Python SDK is used in at least one external training run. Leaderboards exist for ≥ 3 games.
- **Data quality:** 100% of finished matches can be replayed. Exported datasets load in Hugging Face `datasets` without custom code.

## 14. Milestones

| Milestone | Scope | Exit criteria |
|---|---|---|
| **M0 – Foundations** | Cargo workspace, `gfa-core` trait + `DynGame`, conformance kit, CI (fmt, clippy, test), Tic-Tac-Toe | Tic-Tac-Toe passes conformance; benchmarks in CI |
| **M1 – Playable API** | `gfa-server` REST + WS, SQLite/Postgres store, event log, Connect Four, built-in minimax/MCTS/random opponents, OpenAPI | Full match lifecycle via curl; replays rebuild exactly |
| **M2 – MCP + Chess** | `gfa-mcp` (stdio + HTTP), chess engine (shakmaty), UCI adapter + Stockfish, difficulty ladder | Claude completes chess games vs Stockfish levels 1–10 via MCP; illegal moves are recoverable |
| **M3 – Frontend** | React+Phaser shell, library, play, live spectate, replay viewer, generic text renderer, chess/C4/TTT scenes | Human can play and replay all MVP games; live AI-vs-AI viewable |
| **M4 – Training** | PyO3 SDK, Gymnasium/PettingZoo wrappers, VectorEnv, batch REST, exports, ratings, tournaments CLI | Throughput targets met; PPO example trains Connect Four agent that beats level 3 |
| **M5 – Catalog expansion** | Checkers, Othello, Go + KataGo (GTP), 2048 | Each passes conformance and has a calibrated opponent ladder |
| **M6 – Imperfect information** | Hold'em, Battleship, Kuhn/Liar's Dice, Hanabi; omniscient replay view | Hidden-info leakage tests pass; CFR baselines available |

## 15. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Engine processes (Stockfish/KataGo) are heavy under load | Pool with limits, queue with back-pressure, cheaper built-in opponents for training, GPU-optional KataGo config |
| Leaking hidden info through observations or MCP tools | Viewer-scoped `observe`, property tests, `analyze_position` disabled for imperfect-information and eval matches |
| LLM agents cheat by calling analysis tools | Per-match `allow_analysis` flag, recorded in match metadata; leaderboards count only clean matches |
| Game-trait design turns out too narrow (simultaneous, stochastic, n-player) | Model these from M0 (`current_players` vec, chance events, `Viewer`), validate against OpenSpiel's game taxonomy |
| DB write volume from RL training | `persist` levels, batched inserts, optional off-by-default storage for batch endpoints |
| Engine version changes invalidate old replays | `engine_version` stored per match; old engine versions stay loadable behind feature flags or replays are flagged |
| License conflicts with bundled engines | License review gate per engine; GPL-3.0 repo license |

## 16. Open questions

1. Hosted multi-tenant service, or self-hosted only for v1?
2. Should LLM agents be able to register as **remote players** (the server calls them through a webhook) in addition to calling the API themselves?
3. Should Chess960 and other variants be separate `game_id`s or config options of `chess`? (Proposed: config options, with `variant` in the spec.)
4. Is Parquet export needed in M4, or is JSONL enough for the first training users?
5. Real-time or continuous games (e.g. Snake, Pong): which phase, and does that need a tick-based `Game` variant?
6. Should human accounts and ratings share the leaderboards with agents?

## 17. Glossary

- **Seat:** a player position in a match.
- **Observation:** what one seat is allowed to see at a moment in time.
- **Action mask:** boolean vector over the fixed action space marking legal actions.
- **UCI / GTP:** standard text protocols for chess engines and Go engines.
- **MCP:** Model Context Protocol, the standard for exposing tools and resources to LLM agents.
- **Conformance kit:** shared test suite every game must pass.
