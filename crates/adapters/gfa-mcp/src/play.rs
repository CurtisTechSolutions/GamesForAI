use crate::play_types::*;
use crate::{decode, failure, internal, success, McpServer};
use gfa_api_types::{
    ApiError, Assists, ControlRequest, CreateMatch, MoveRequest, OpponentConfig, Seat, Start,
};
use gfa_core::Viewer;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

impl McpServer {
    pub(crate) async fn play(&self, name: &str, arguments: Value) -> CallToolResult {
        self.play_result(name, arguments)
            .await
            .unwrap_or_else(failure)
    }
    pub(crate) fn player(&self) -> Result<u8, ApiError> {
        match self.viewer {
            Viewer::Player(seat) => Ok(seat),
            _ => Err(ApiError::new(
                "FORBIDDEN",
                "A spectator cannot control a player",
                "Connect with a host-authorized player seat.",
            )),
        }
    }
    async fn play_result(&self, name: &str, arguments: Value) -> Result<CallToolResult, ApiError> {
        match name {
            "create_match" => self.create_game(decode(arguments)?).await,
            "make_move" => self.move_game(decode(arguments)?).await,
            "get_state" | "get_legal_actions" | "resign" => {
                let args: MatchArgs = decode(arguments)?;
                let state = self.service.get_state(&args.match_id, self.viewer).await?;
                if name == "resign" {
                    let seat = self.player()?;
                    let state = self
                        .service
                        .resign(
                            &args.match_id,
                            ControlRequest {
                                seat,
                                turn: state.turn,
                            },
                        )
                        .await?;
                    let output = StateOutput::from_state(state, self.viewer)?;
                    return success(&output, output.text());
                }
                let output = StateOutput::from_state(state, self.viewer)?;
                if name == "get_legal_actions" {
                    let notation = "Copy a canonical string exactly into make_move.action; do not infer an action from the board.".to_string();
                    let text = format!(
                        "Turn {}. To act: {:?}.\n{}\n{}",
                        output.turn,
                        output.to_act,
                        notation,
                        output.legal_actions.join(", ")
                    );
                    success(
                        LegalOutput {
                            match_id: output.match_id,
                            turn: output.turn,
                            to_act: output.to_act,
                            legal_actions: output.legal_actions,
                            notation,
                        },
                        text,
                    )
                } else {
                    success(&output, output.text())
                }
            }
            _ => Err(internal()),
        }
    }
    async fn create_game(&self, args: CreateArgs) -> Result<CallToolResult, ApiError> {
        let seat = self.player()?;
        if args.play_as.is_some_and(|requested| requested != seat) {
            return Err(ApiError::new(
                "FORBIDDEN",
                "Requested seat differs from the host-authorized seat",
                "Omit play_as or connect with the requested player seat.",
            ));
        }
        if args.opponent.is_none() && args.level.is_some() {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "A level requires an opponent",
                "Select an installed opponent or omit level.",
            ));
        }
        let spec = self
            .service
            .list_games()
            .into_iter()
            .find(|game| game.id == args.game_id)
            .ok_or_else(|| {
                ApiError::new(
                    "UNKNOWN_GAME",
                    "Game is not installed",
                    "Choose a game from list_games.",
                )
            })?;
        let seats = if let Some(id) = args.opponent {
            if spec.num_players != [2, 2] || seat > 1 {
                return Err(ApiError::new(
                    "INVALID_CONFIG",
                    "Automatic opposition requires a two-player game",
                    "Omit opponent for this game.",
                ));
            }
            let mut seats = vec![Seat::SelfPlayer; 2];
            seats[usize::from(1 - seat)] = Seat::Opponent {
                opponent: OpponentConfig {
                    id,
                    level: args.level,
                    limits: Default::default(),
                    uci: None,
                },
                seed: None,
            };
            seats
        } else {
            vec![]
        };
        let created = self
            .service
            .create_match_with_info(
                CreateMatch {
                    game_id: args.game_id,
                    config: args.config.unwrap_or(json!({})),
                    seed: args.seed,
                    start: args.start.map(|position| Start::Position { position }),
                    seats,
                    assists: Assists {
                        allow_analysis: args.allow_analysis,
                        allow_simulation: args.allow_simulation,
                    },
                    include_info: true,
                },
                self.viewer,
            )
            .await?;
        let state = StateOutput::from_state(created.state, self.viewer)?;
        let info = crate::briefing::compact(created.info.ok_or_else(internal)?, self.viewer)?;
        let text = format!(
            "{}\n\n{}",
            info.markdown().map_err(|_| internal())?,
            state.text()
        );
        success(
            CreatedOutput {
                state,
                info: info.into(),
            },
            text,
        )
    }
    async fn move_game(&self, args: MoveArgs) -> Result<CallToolResult, ApiError> {
        let seat = self.player()?;
        if args.idempotency_key.is_some() && args.turn.is_none() {
            return Err(ApiError::new(
                "INVALID_REQUEST",
                "Idempotent moves require their original turn",
                "Supply the turn from get_state together with idempotency_key.",
            ));
        }
        let turn = if let Some(turn) = args.turn {
            turn
        } else {
            self.service
                .get_state(&args.match_id, self.viewer)
                .await?
                .turn
        };
        let moved = self
            .service
            .make_move(
                &args.match_id,
                MoveRequest {
                    seat,
                    turn,
                    action: args.action,
                    reasoning: args.reasoning,
                },
                args.idempotency_key.as_deref(),
            )
            .await?;
        let output = MovedOutput {
            accepted_action: moved.accepted_action.string,
            opponent_actions: moved
                .opponent_actions
                .into_iter()
                .map(|reply| Reply {
                    seat: reply.seat,
                    turn: reply.turn,
                    action: reply.action.map(|action| action.string),
                })
                .collect(),
            state: StateOutput::from_state(moved.state, self.viewer)?,
        };
        let text = format!(
            "Accepted: {}\nOpponent replies: {}\n{}",
            output.accepted_action,
            output
                .opponent_actions
                .iter()
                .map(|reply| format!(
                    "seat {}: {}",
                    reply.seat,
                    reply.action.as_deref().unwrap_or("private action")
                ))
                .collect::<Vec<_>>()
                .join("; "),
            output.state.text()
        );
        success(output, text)
    }
}
