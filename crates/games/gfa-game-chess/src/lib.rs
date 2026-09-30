//! Standard chess with validated FEN imports and replayable draw history.
use gfa_core::schemars::JsonSchema;
use gfa_core::serde::{Deserialize, Serialize};
use gfa_core::serde_json::{self, json};
use gfa_core::{
    schema, ErrorCode, Game, GameError, GameSpec, Information, Observation, PlayerId, StepEvents,
    Tensor, TurnStructure, Viewer,
};
use shakmaty::{
    fen::Fen, san::SanPlus, uci::UciMove, CastlingMode, Chess, Color, EnPassantMode, Position,
    Square,
};
use std::{collections::BTreeMap, sync::OnceLock};

mod encoding;

const MAX_PLIES: u32 = 1000;

/// Standard chess configuration. Time controls belong to the match service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", default, deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Config {
    /// Optional six-field FEN, otherwise the standard starting position.
    pub start_fen: Option<String>,
    /// Truncate after this many actions (1..=1000), with zero returns.
    #[schemars(range(min = 1, max = 1000))]
    pub max_plies: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            start_fen: None,
            max_plies: MAX_PLIES,
        }
    }
}

/// A UCI move or a draw claim, optionally identifying the intended next move.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "gfa_core::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "gfa_core::schemars")]
pub enum Action {
    /// Move a piece, including castling or promotion.
    Move {
        /// Lowercase UCI, such as e2e4, e1g1, or a7a8n.
        uci: String,
    },
    /// Claim a threefold or fifty-move draw.
    ClaimDraw {
        /// A legal intended UCI move that would establish the claim, or null
        /// to claim the current position. A valid claim ends without playing it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intended: Option<String>,
    },
}

/// Complete public state. The cached board is derived and never trusted on import.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct State {
    config: Config,
    initial_fen: String,
    actions: Vec<Action>,
    #[serde(skip)]
    #[schemars(skip)]
    cache: OnceLock<Result<Derived, GameError>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", tag = "type", rename_all = "snake_case")]
#[schemars(crate = "gfa_core::schemars")]
enum Status {
    Active,
    Checkmate { winner: PlayerId },
    Stalemate,
    InsufficientMaterial,
    FivefoldRepetition,
    SeventyFiveMove,
    ClaimedDraw,
    MoveLimit,
}

#[derive(Clone, Debug)]
struct Derived {
    position: Chess,
    repetitions: BTreeMap<String, u8>,
    status: Status,
    last_move_san: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
struct BoardView {
    state: State,
    fen: String,
    pieces: BTreeMap<String, String>,
    to_move: PlayerId,
    check: bool,
    halfmoves: u32,
    fullmoves: u32,
    repetitions: u8,
    can_claim_draw: bool,
    status: Status,
    last_move_san: Option<String>,
}

/// Stateless standard-chess engine.
pub struct ChessGame;

fn seat(color: Color) -> PlayerId {
    u8::from(color == Color::Black)
}

fn parse_fen(text: &str) -> Result<Chess, GameError> {
    if text.len() > 128 || text.split_whitespace().count() != 6 {
        return Err(GameError::position(
            "Expected six FEN fields, at most 128 bytes",
        ));
    }
    let fen: Fen = text
        .parse()
        .map_err(|e| GameError::position(format!("Invalid FEN: {e}")))?;
    fen.into_position(CastlingMode::Standard)
        .map_err(|e| GameError::position(format!("Invalid chess position: {e}")))
}

fn fen(position: &Chess) -> String {
    Fen::from_position(position, EnPassantMode::Always).to_string()
}

// Legal en passant, side to move, pieces and castling rights define repetition.
// Counters are deliberately excluded. Full strings avoid probabilistic hash collisions.
fn key(position: &Chess) -> String {
    Fen::from_position(position, EnPassantMode::Legal)
        .to_string()
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

fn count(derived: &Derived) -> u8 {
    derived
        .repetitions
        .get(&key(&derived.position))
        .copied()
        .unwrap_or(0)
}

fn classify(position: &Chess, repetitions: u8, plies: usize, max_plies: u32) -> Status {
    if position.legal_moves().is_empty() {
        return if position.is_check() {
            Status::Checkmate {
                winner: 1 - seat(position.turn()),
            }
        } else {
            Status::Stalemate
        };
    }
    if position.is_insufficient_material() {
        return Status::InsufficientMaterial;
    }
    if repetitions >= 5 {
        return Status::FivefoldRepetition;
    }
    if position.halfmoves() >= 150 {
        return Status::SeventyFiveMove;
    }
    if plies >= max_plies as usize {
        return Status::MoveLimit;
    }
    Status::Active
}

fn parse_uci(text: &str) -> Result<UciMove, GameError> {
    let uci: UciMove = text.parse().map_err(|_| notation_error())?;
    if !uci.is_normal() || uci.to_string() != text || encoding::move_index(text).is_none() {
        return Err(notation_error());
    }
    Ok(uci)
}

fn notation_error() -> GameError {
    GameError::new(
        ErrorCode::UnparseableAction,
        "Expected lowercase UCI (e2e4, a7a8q), claim_draw, or claim_draw:<UCI>",
        "Choose a listed legal action; SAN and null moves are not accepted.",
    )
}

fn claimable(derived: &Derived, intended: Option<&str>) -> Result<bool, GameError> {
    if let Some(text) = intended {
        let m = parse_uci(text)?
            .to_move(&derived.position)
            .map_err(|_| GameError::illegal("The intended draw-claim move is illegal"))?;
        let mut next = derived.position.clone();
        next.play_unchecked(m);
        if UciMove::from_standard(m).to_string() != text {
            return Err(GameError::illegal(
                "Use canonical standard castling notation",
            ));
        }
        Ok(next.halfmoves() >= 100
            || derived.repetitions.get(&key(&next)).copied().unwrap_or(0) >= 2)
    } else {
        Ok(derived.position.halfmoves() >= 100 || count(derived) >= 3)
    }
}

fn advance(
    derived: &mut Derived,
    action: &Action,
    plies: usize,
    config: &Config,
) -> Result<(), GameError> {
    if derived.status != Status::Active {
        return Err(GameError::new(
            ErrorCode::MatchFinished,
            "The chess game has ended",
            "Create or fork a match.",
        ));
    }
    match action {
        Action::Move { uci } => {
            let m = parse_uci(uci)?
                .to_move(&derived.position)
                .map_err(|_| GameError::illegal("The UCI move is not legal in this position"))?;
            // UciMove also accepts king-to-rook castling aliases; canonical standard
            // chess uses king-to-destination notation only.
            if UciMove::from_standard(m).to_string() != *uci {
                return Err(GameError::illegal(
                    "Use e1g1/e1c1 or e8g8/e8c8 for standard castling",
                ));
            }
            derived.last_move_san =
                Some(SanPlus::from_move_and_play_unchecked(&mut derived.position, m).to_string());
            let repetitions = derived
                .repetitions
                .entry(key(&derived.position))
                .or_default();
            *repetitions += 1;
            derived.status = classify(&derived.position, *repetitions, plies, config.max_plies);
        }
        Action::ClaimDraw { intended } => {
            if !claimable(derived, intended.as_deref())? {
                return Err(GameError::illegal(
                    "Neither a threefold repetition nor fifty-move claim is available",
                ));
            }
            derived.status = Status::ClaimedDraw;
        }
    }
    Ok(())
}

impl State {
    fn derive(&self) -> Result<Derived, GameError> {
        validate_config(&self.config)?;
        if self.actions.len() > self.config.max_plies as usize {
            return Err(GameError::position("Chess history exceeds max_plies"));
        }
        let position = parse_fen(&self.initial_fen)?;
        if fen(&position) != self.initial_fen {
            return Err(GameError::position("Stored initial_fen must be canonical"));
        }
        let status = classify(&position, 1, 0, self.config.max_plies);
        let mut derived = Derived {
            repetitions: BTreeMap::from([(key(&position), 1)]),
            position,
            status,
            last_move_san: None,
        };
        for (index, action) in self.actions.iter().enumerate() {
            advance(&mut derived, action, index + 1, &self.config).map_err(|e| {
                GameError::position(format!(
                    "Invalid history at action {}: {}",
                    index + 1,
                    e.message
                ))
            })?;
        }
        Ok(derived)
    }

    fn derived(&self) -> Result<&Derived, GameError> {
        self.cache
            .get_or_init(|| self.derive())
            .as_ref()
            .map_err(Clone::clone)
    }
}

fn validate_config(config: &Config) -> Result<(), GameError> {
    if !(1..=MAX_PLIES).contains(&config.max_plies) {
        return Err(GameError::new(
            ErrorCode::InvalidConfig,
            "max_plies must be 1..=1000",
            "Use the standard default of 1000 or a smaller training cap.",
        ));
    }
    if let Some(start) = &config.start_fen {
        parse_fen(start)?;
    }
    Ok(())
}

impl Game for ChessGame {
    type State = State;
    type Action = Action;
    type Config = Config;

    fn spec() -> GameSpec {
        GameSpec {
            id: "chess".into(), name: "Chess".into(),
            summary: "Standard chess: checkmate, castling, en passant, promotions and explicit draw claims.".into(),
            engine_version: env!("CARGO_PKG_VERSION").into(), info_version: "1.0.0".into(),
            num_players: [2, 2], seat_names: vec!["White".into(), "Black".into()],
            turn_structure: TurnStructure::Sequential, information: Information::Perfect,
            stochastic: false, max_game_length: MAX_PLIES, reward_range: [-1.0, 1.0],
            action_space_size: encoding::ACTION_SPACE,
            rules_markdown: include_str!("rules.md").into(),
            action_notation: "Lowercase UCI: e2e4, e1g1 (castling), a7a8q/r/b/n (promotion). Draw claims: claim_draw or claim_draw:e2e4. Indices: 64 origin squares * 73 move planes, 4672 for current-position claim, 4673 + move index for intended-move claim; use the legal list.".into(),
            position_notation: "Six-field FEN imports a board with no earlier repetition history. Pristine exports are FEN; played-state exports are lossless JSON {config,initial_fen,actions}. public_position always gives current FEN.".into(),
            config_schema: schema::<Config>(), action_schema: schema::<Action>(),
            observation_schema: schema::<BoardView>(),
        }
    }

    fn play_guide() -> Option<gfa_core::PlayGuide> {
        Some(gfa_core::PlayGuide {
            objective: "Checkmate wins. Stalemate and insufficient mating material draw. Threefold repetition and 50 moves per side without a pawn move or capture allow an explicit claim; fivefold and 75 moves per side draw automatically. Checkmate takes precedence over automatic move-count draws.".into(),
            observation: "Files a..h run left to right; ranks 8..1 top to bottom. Uppercase pieces are White (seat 0); lowercase Black (seat 1); . is empty. JSON includes FEN, a square-to-piece map, check, counters, status, last SAN move and the complete public starting position/action history, needed to reproduce draw claims.".into(),
            tensor: "Shape [20,8,8], rank 8 first. Planes 0..5 White P,N,B,R,Q,K; 6..11 Black; 12 White-to-move; 13..16 K,Q,k,q rights; 17 legal en-passant target; 18 halfmove count/150 capped at 1; 19 current repetition count/5 capped at 1. History remains in JSON.".into(),
            rewards: "Only checkmate pays +1 to the winner and -1 to the loser. Draws, unfinished states and the max_plies truncation pay 0. No shaped rewards.".into(),
            config: "start_fen optionally replaces the standard board with a validated six-field FEN. max_plies is 1..1000 (default 1000) and caps actions from the imported start. Standard chess only; clocks and resign/draw agreement are match controls.".into(),
            solved: "Standard chess is not solved.".into(),
            common_mistakes: vec!["Use UCI e2e4, not SAN e4. Use lowercase promotion letters including underpromotions.".into(), "You cannot castle through check, and en passant is available for one move only.".into(), "A FEN alone cannot preserve earlier repetitions; use the complete JSON export to resume history.".into(), "Claim a draw only when claim_draw or the intended-move claim appears in legal_actions.".into()],
            strategy_notes: vec!["Check immediate threats to both kings before committing a move.".into()],
        })
    }

    fn new_initial_state(config: &Config, _: u64) -> Result<State, GameError> {
        validate_config(config)?;
        let position = match &config.start_fen {
            Some(text) => parse_fen(text)?,
            None => Chess::default(),
        };
        let state = State {
            config: config.clone(),
            initial_fen: fen(&position),
            actions: vec![],
            cache: OnceLock::new(),
        };
        Self::validate_state(&state)?;
        Ok(state)
    }

    fn validate_state(state: &State) -> Result<(), GameError> {
        state.derived().map(|_| ())
    }

    fn current_players(state: &State) -> Vec<PlayerId> {
        match state.derived() {
            Ok(d) if d.status == Status::Active => vec![seat(d.position.turn())],
            _ => vec![],
        }
    }

    fn legal_actions(state: &State, player: PlayerId) -> Vec<Action> {
        let Ok(d) = state.derived() else {
            return vec![];
        };
        if d.status != Status::Active || player != seat(d.position.turn()) {
            return vec![];
        }
        let moves = d.position.legal_moves();
        let mut actions: Vec<_> = moves
            .iter()
            .map(|&m| Action::Move {
                uci: UciMove::from_standard(m).to_string(),
            })
            .collect();
        if claimable(d, None).unwrap_or(false) {
            actions.push(Action::ClaimDraw { intended: None });
        }
        if d.position.halfmoves() >= 99 || d.repetitions.values().any(|&n| n >= 2) {
            for &m in &moves {
                let uci = UciMove::from_standard(m).to_string();
                if claimable(d, Some(&uci)).unwrap_or(false) {
                    actions.push(Action::ClaimDraw {
                        intended: Some(uci),
                    });
                }
            }
        }
        actions
    }

    fn apply(
        state: &mut State,
        player: PlayerId,
        action: &Action,
    ) -> Result<StepEvents, GameError> {
        let d = state.derived()?;
        if d.status != Status::Active {
            return Err(GameError::new(
                ErrorCode::MatchFinished,
                "The chess game has ended",
                "Create or fork a match.",
            ));
        }
        if player != seat(d.position.turn()) {
            return Err(GameError::new(
                ErrorCode::NotYourTurn,
                "The other color must move",
                "Wait for your turn.",
            ));
        }
        let d = state
            .cache
            .get_mut()
            .ok_or_else(|| GameError::position("Missing derived board"))?
            .as_mut()
            .map_err(|e| e.clone())?;
        advance(d, action, state.actions.len() + 1, &state.config)?;
        state.actions.push(action.clone());
        Ok(StepEvents {
            truncated: d.status == Status::MoveLimit,
            events: vec![],
        })
    }

    fn is_terminal(state: &State) -> bool {
        state
            .derived()
            .is_ok_and(|d| !matches!(d.status, Status::Active | Status::MoveLimit))
    }

    fn returns(state: &State) -> Vec<f64> {
        match state.derived().map(|d| &d.status) {
            Ok(Status::Checkmate { winner: 0 }) => vec![1.0, -1.0],
            Ok(Status::Checkmate { winner: 1 }) => vec![-1.0, 1.0],
            _ => vec![0.0, 0.0],
        }
    }

    fn observe(state: &State, _: Viewer) -> Observation {
        let Ok(d) = state.derived() else {
            return Observation {
                text: "Invalid chess position.".into(),
                json: json!(null),
                tensor: None,
            };
        };
        let mut text = String::from("  a b c d e f g h\n");
        let mut pieces = BTreeMap::new();
        let mut values = vec![0.0; 20 * 64];
        for row in 0..8 {
            text.push_str(&format!("{} ", 8 - row));
            for col in 0..8 {
                let square = Square::new(((7 - row) * 8 + col) as u32);
                let ch = if let Some(piece) = d.position.board().piece_at(square) {
                    pieces.insert(square.to_string(), piece.char().to_string());
                    let role = match piece.role {
                        shakmaty::Role::Pawn => 0,
                        shakmaty::Role::Knight => 1,
                        shakmaty::Role::Bishop => 2,
                        shakmaty::Role::Rook => 3,
                        shakmaty::Role::Queen => 4,
                        shakmaty::Role::King => 5,
                    };
                    values[(seat(piece.color) as usize * 6 + role) * 64 + row * 8 + col] = 1.0;
                    piece.char()
                } else {
                    '.'
                };
                text.push(ch);
                text.push(' ');
            }
            text.push('\n');
        }
        let current_fen = fen(&d.position);
        let rights = current_fen.split_whitespace().nth(2).unwrap_or("-");
        let reps = count(d);
        for i in 0..64 {
            values[12 * 64 + i] = if d.position.turn() == Color::White {
                1.0
            } else {
                0.0
            };
            for (plane, flag) in ['K', 'Q', 'k', 'q'].into_iter().enumerate() {
                values[(13 + plane) * 64 + i] = if rights.contains(flag) { 1.0 } else { 0.0 };
            }
            values[18 * 64 + i] = (d.position.halfmoves() as f32 / 150.0).min(1.0);
            values[19 * 64 + i] = (f32::from(reps) / 5.0).min(1.0);
        }
        if let Some(ep) = d.position.legal_ep_square() {
            let i = ep as usize;
            values[17 * 64 + (7 - i / 8) * 8 + i % 8] = 1.0;
        }
        text.push_str(&format!(
            "{:?}; {} to move (seat {}).{}",
            d.status,
            if d.position.turn() == Color::White {
                "White"
            } else {
                "Black"
            },
            seat(d.position.turn()),
            if d.position.is_check() { " Check." } else { "" }
        ));
        Observation {
            text,
            json: json!(BoardView {
                state: state.clone(),
                fen: current_fen,
                pieces,
                to_move: seat(d.position.turn()),
                check: d.position.is_check(),
                halfmoves: d.position.halfmoves(),
                fullmoves: d.position.fullmoves().get(),
                repetitions: reps,
                can_claim_draw: d.status == Status::Active && claimable(d, None).unwrap_or(false),
                status: d.status.clone(),
                last_move_san: d.last_move_san.clone(),
            }),
            tensor: Some(Tensor {
                shape: vec![20, 8, 8],
                values,
            }),
        }
    }

    fn state_from_observation(
        config: &Config,
        observation: &Observation,
        _: Viewer,
        _: u64,
    ) -> Result<State, GameError> {
        let view: BoardView = serde_json::from_value(observation.json.clone())
            .map_err(|e| GameError::position(e.to_string()))?;
        if &view.state.config != config {
            return Err(GameError::position("Observation config does not match"));
        }
        Self::validate_state(&view.state)?;
        if Self::observe(&view.state, Viewer::Spectator).json != observation.json {
            return Err(GameError::position(
                "Observation fields disagree with chess history",
            ));
        }
        Ok(view.state)
    }

    fn action_to_string(_: &State, action: &Action) -> String {
        match action {
            Action::Move { uci } => uci.clone(),
            Action::ClaimDraw { intended: None } => "claim_draw".into(),
            Action::ClaimDraw {
                intended: Some(uci),
            } => format!("claim_draw:{uci}"),
        }
    }

    fn action_from_string(_: &State, text: &str) -> Result<Action, GameError> {
        if text == "claim_draw" {
            return Ok(Action::ClaimDraw { intended: None });
        }
        if let Some(uci) = text.strip_prefix("claim_draw:") {
            parse_uci(uci)?;
            return Ok(Action::ClaimDraw {
                intended: Some(uci.into()),
            });
        }
        parse_uci(text)?;
        Ok(Action::Move { uci: text.into() })
    }

    fn action_to_index(action: &Action) -> u32 {
        encoding::index(action)
    }

    fn action_from_index(state: &State, index: u32) -> Result<Action, GameError> {
        Self::current_players(state)
            .first()
            .and_then(|&seat| {
                Self::legal_actions(state, seat)
                    .into_iter()
                    .find(|a| Self::action_to_index(a) == index)
            })
            .ok_or_else(|| GameError::illegal("No legal chess action has that index"))
    }

    fn public_position(state: &State) -> Result<Option<String>, GameError> {
        Ok(Some(fen(&state.derived()?.position)))
    }

    fn state_to_notation(state: &State) -> Result<String, GameError> {
        Self::validate_state(state)?;
        if state.actions.is_empty() {
            Ok(state.initial_fen.clone())
        } else {
            Ok(serde_json::to_string(state)?)
        }
    }

    fn state_from_notation(config: &Config, notation: &str) -> Result<State, GameError> {
        validate_config(config)?;
        let state: State = if notation.trim_start().starts_with('{') {
            if notation.len() > 128 * 1024 {
                return Err(GameError::position("Chess history exceeds 128 KiB"));
            }
            serde_json::from_str(notation).map_err(|e| GameError::position(e.to_string()))?
        } else {
            State {
                config: config.clone(),
                initial_fen: fen(&parse_fen(notation)?),
                actions: vec![],
                cache: OnceLock::new(),
            }
        };
        if &state.config != config {
            return Err(GameError::position("Imported chess config does not match"));
        }
        Self::validate_state(&state)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests;
