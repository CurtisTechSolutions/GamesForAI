import type { MatchState } from "@gfa/api-client";

export function resultLabel(state: MatchState, names: string[] = []) {
  if (state.truncated) return "Move limit reached";
  if (state.outcome?.reason === "resigned")
    return (
      (names[state.outcome.seat] ?? "Seat " + state.outcome.seat) + " resigned"
    );
  if (state.outcome?.reason === "agreed_draw") return "Draw by agreement";
  if (state.terminated) {
    const winner = state.returns.findIndex((value) => value > 0);
    return winner >= 0
      ? (names[winner] ?? "Seat " + winner) +
          (state.returns.length === 1 ? " completed the puzzle" : " wins")
      : state.returns.length === 1
        ? "Puzzle finished"
        : "Draw";
  }
  return (
    state.to_act.map((index) => names[index] ?? "Seat " + index).join(", ") +
    " to move"
  );
}
