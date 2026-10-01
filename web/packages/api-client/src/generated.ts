// Generated from the Rust OpenAPI document. Do not edit by hand.
// Regenerate with .github/scripts/generate-api-types.py.
// prettier-ignore
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

// prettier-ignore
export interface Schemas {
  "ActionChoice": { "action": Schemas["LegalAction"]; "info": Schemas["ChoiceInfo"]; };
  "AdviceInfo": { "details": JsonValue; "is_guess": boolean; "summary": string; "technique": string; };
  "AnalysisRequest": { "config"?: JsonValue; "from": Schemas["SimulationFrom"]; "game_id": string; "opponent": Schemas["OpponentConfig"]; "seed"?: number | null; };
  "AnalysisResult": { "advice"?: (null) | (Schemas["AdviceInfo"]); "best_moves": Array<Schemas["LegalAction"]>; "budget_exhausted": boolean; "depth": number; "evaluation"?: number | null; "game_id": string; "nodes": number; "opponent": string; "principal_variation": Array<string>; "seed": number; "variations"?: Array<Schemas["ActionChoice"]>; };
  "ApiError": { "code": string; "details": JsonValue; "hint": string; "message": string; };
  "AssistUsage": { "seat": number; "simulated_moves": number; "simulation_calls": number; };
  "Assists": { "allow_analysis"?: boolean; "allow_simulation"?: boolean; };
  "Briefing": { "approx_tokens": number; "info_version": string; "sections": Array<Schemas["InfoSection"]>; };
  "ChoiceInfo": { "advice"?: (null) | (Schemas["AdviceInfo"]); "algorithm": string; "budget_exhausted": boolean; "depth": number; "evaluation"?: number | null; "nodes": number; "principal_variation": Array<string>; };
  "ControlRequest": { "seat": number; "turn": number; };
  "CreateMatch": { "assists"?: Schemas["Assists"]; "config"?: JsonValue; "game_id": string; "include_info"?: boolean; "seats"?: Array<Schemas["Seat"]>; "seed"?: number | null; "start"?: (null) | (Schemas["Start"]); };
  "CreatedMatch": (Schemas["MatchState"]) & ({ "info"?: (null) | (Schemas["Briefing"]); });
  "EnvSnapshot": { "config": JsonValue; "engine_version": string; "format_version": number; "game_id": string; "state": JsonValue; };
  "ErrorResponse": { "error": Schemas["ApiError"]; };
  "EventData": ({ "assists": Schemas["Assists"]; "at_ms": number; "config": JsonValue; "engine_version": string; "game_id": string; "initial": Schemas["MatchState"]; "initial_state"?: JsonValue; "seats": Array<Schemas["Seat"]>; "seed"?: number | null; "type": "created"; }) | ({ "action"?: (null) | (Schemas["LegalAction"]); "at_ms": number; "events": Array<Schemas["GameEvent"]>; "opponent_info"?: JsonValue; "reasoning"?: string | null; "seat": number; "turn": number; "type": "action"; }) | ({ "source": Schemas["ForkSource"]; "type": "forked_from"; }) | ({ "at_ms": number; "seat": number; "turn": number; "type": "resigned"; }) | ({ "at_ms": number; "seat": number; "turn": number; "type": "draw_offered"; }) | ({ "returns": Array<number>; "terminated": boolean; "truncated": boolean; "turn": number; "type": "finished"; });
  "EventPage": { "events": Array<Schemas["RecordedEvent"]>; "match_id": string; "next"?: number | null; "revision": number; };
  "ForkMatch": { "include_info"?: boolean; "keep_rng"?: boolean; "seats"?: Array<Schemas["Seat"]> | null; "seed"?: number | null; "turn": number; };
  "ForkSource": { "match_id": string; "turn": number; };
  "GameEvent": { "kind": string; "payload": JsonValue; "visible_to"?: Array<Schemas["u8"]> | null; };
  "GameInfoQuery": { "config"?: string | null; "detail"?: Schemas["InfoDetail"]; "format"?: Schemas["InfoFormat"]; "seat"?: number | null; };
  "GameSpec": { "action_notation": string; "action_schema": JsonValue; "action_space_size": number; "config_schema": JsonValue; "engine_version": string; "id": string; "info_version": string; "information": Schemas["Information"]; "max_game_length": number; "name": string; "num_players": Array<number>; "observation_schema": JsonValue; "position_notation": string; "reward_range": Array<number>; "rules_markdown": string; "seat_names": Array<string>; "stochastic": boolean; "summary": string; "turn_structure": Schemas["TurnStructure"]; };
  "Health": { "mode": string; "status": string; };
  "InfoDetail": "compact" | "full";
  "InfoFormat": "json" | "markdown";
  "InfoSection": { "data": JsonValue; "id": string; "text": string; };
  "Information": "perfect" | "imperfect";
  "LegalAction": { "index": number; "json": JsonValue; "string": string; };
  "LegalActions": { "action_mask": Array<boolean>; "legal_actions": Array<Schemas["LegalAction"]>; "to_act": Array<Schemas["u8"]>; "turn": number; };
  "MatchHistory": { "matches": Array<Schemas["MatchMetadata"]>; "next"?: string | null; };
  "MatchInfoQuery": { "detail"?: Schemas["InfoDetail"]; "format"?: Schemas["InfoFormat"]; "seat"?: number | null; };
  "MatchMetadata": { "assist_usage": Array<Schemas["AssistUsage"]>; "assists": Schemas["Assists"]; "created_at_ms": number; "draw_offer"?: number | null; "engine_version": string; "forked_from"?: (null) | (Schemas["ForkSource"]); "game_id": string; "match_id": string; "outcome"?: (null) | (Schemas["MatchOutcome"]); "returns": Array<number>; "seats"?: Array<Schemas["Seat"]>; "status": Schemas["MatchStatus"]; "terminated": boolean; "to_act": Array<number>; "truncated": boolean; "turn": number; };
  "MatchOutcome": ({ "reason": "resigned"; "seat": number; }) | ({ "reason": "agreed_draw"; });
  "MatchState": { "action_mask": Array<boolean>; "draw_offer"?: number | null; "legal_actions": Array<Schemas["LegalAction"]>; "match_id": string; "observation": Schemas["Observation"]; "outcome"?: (null) | (Schemas["MatchOutcome"]); "returns": Array<number>; "terminated": boolean; "to_act": Array<Schemas["u8"]>; "truncated": boolean; "turn": number; };
  "MatchStatus": "active" | "finished";
  "MoveRequest": { "action": JsonValue; "reasoning"?: string | null; "seat": number; "turn": number; };
  "MoveResult": { "accepted_action": Schemas["LegalAction"]; "opponent_actions"?: Array<Schemas["OpponentReply"]>; "state": Schemas["MatchState"]; };
  "Observation": { "json": JsonValue; "tensor"?: (null) | (Schemas["Tensor"]); "text": string; };
  "OpponentConfig": { "id": string; "level"?: number | null; "limits"?: Schemas["SearchBudget"]; "uci"?: (null) | (Schemas["UciOptions"]); };
  "OpponentLevel": { "level": number; "rating"?: number | null; };
  "OpponentReply": { "action"?: (null) | (Schemas["LegalAction"]); "seat": number; "turn": number; };
  "OpponentSpec": { "calibrated": boolean; "id": string; "levels": Array<Schemas["OpponentLevel"]>; "name": string; };
  "RecordedEvent": (Schemas["EventData"]) & ({ "sequence": number; });
  "Replay": { "ancestors"?: Array<Schemas["ReplayAncestor"]>; "config": JsonValue; "engine_version": string; "events": Array<Schemas["RecordedEvent"]>; "game_id": string; "initial_state"?: JsonValue; "match_id": string; "revision": number; "seed"?: number | null; "states": Array<Schemas["MatchState"]>; };
  "ReplayAncestor": { "engine_version": string; "source": Schemas["ForkSource"]; "states": Array<Schemas["MatchState"]>; };
  "SearchBudget": { "depth"?: number | null; "nodes"?: number | null; "time_ms"?: number | null; };
  "Seat": ({ "type": "self"; }) | ({ "type": "human"; }) | ({ "type": "open"; }) | ({ "opponent": Schemas["OpponentConfig"]; "seed"?: number | null; "type": "opponent"; });
  "SimulateRequest": { "config"?: JsonValue; "from": Schemas["SimulationFrom"]; "lines": Array<Array<JsonValue>>; "return"?: Schemas["SimulationOutput"]; "seed"?: number | null; };
  "SimulatedLine": { "error"?: (null) | (Schemas["SimulationFailure"]); "moves_applied": number; "states": Array<Schemas["MatchState"]>; };
  "SimulationFailure": { "error": Schemas["ApiError"]; "index": number; };
  "SimulationFrom": ({ "match_id": string; "seat": number; "turn"?: number | null; }) | ({ "position": string; }) | ({ "state": JsonValue; });
  "SimulationOutput": "final" | "all";
  "SimulationResult": { "game_id": string; "lines": Array<Schemas["SimulatedLine"]>; "seed": number; };
  "Start": ({ "position": string; }) | ({ "state": JsonValue; });
  "StreamMessage": ({ "state": Schemas["MatchState"]; "type": "state"; }) | ({ "error": Schemas["ApiError"]; "type": "error"; });
  "Tensor": { "shape": Array<number>; "values": Array<number>; };
  "TrainingBatch": { "operations": Array<Schemas["TrainingOperation"]>; };
  "TrainingBatchResult": { "results": Array<Schemas["TrainingResult"]>; };
  "TrainingFrame": { "action_mask": Array<boolean>; "legal_actions": Array<Schemas["LegalAction"]>; "observation": Schemas["Observation"]; "returns": Array<number>; "rewards": Array<number>; "seat"?: (null) | (Schemas["u8"]); "terminated": boolean; "to_act": Array<Schemas["u8"]>; "truncated": boolean; };
  "TrainingOperation": ({ "config"?: JsonValue; "game_id": string; "op": "create"; "position"?: string | null; "seat": Schemas["u8"]; "seed": number; }) | ({ "checkpoint": Schemas["EnvSnapshot"]; "op": "reset"; "position"?: string | null; "seat": Schemas["u8"]; "seed": number; }) | ({ "action": number; "checkpoint": Schemas["EnvSnapshot"]; "op": "step"; "seat": Schemas["u8"]; }) | ({ "checkpoint": Schemas["EnvSnapshot"]; "op": "observe"; "seat"?: (null) | (Schemas["u8"]); });
  "TrainingResult": ({ "checkpoint": Schemas["EnvSnapshot"]; "frame": Schemas["TrainingFrame"]; "status": "ok"; }) | ({ "error": Schemas["ApiError"]; "status": "error"; });
  "TurnStructure": "sequential" | "simultaneous";
  "UciOptions": { "elo"?: number | null; "multipv"?: number | null; "skill"?: number | null; };
  "ValidatePosition": { "config"?: JsonValue; "seed"?: number | null; "start": Schemas["Start"]; };
  "ValidatedPosition": { "action_mask": Array<boolean>; "config": JsonValue; "engine_version": string; "game_id": string; "legal_actions": Array<Schemas["LegalAction"]>; "observation": Schemas["Observation"]; "position": string; "returns": Array<number>; "seed": number; "state": JsonValue; "terminated": boolean; "to_act": Array<Schemas["u8"]>; };
  "u8": number;
}
