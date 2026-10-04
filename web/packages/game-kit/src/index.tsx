import { Component, lazy, Suspense, useId } from "react";
import type { ReactNode } from "react";
import type { BoardProps } from "./types";
export type { BoardProps } from "./types";
import { hasGameScene } from "./registry";

const BoardHost = lazy(() => import("./board-host"));
class BoardBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    return this.state.failed ? (
      <p role="status">
        The visual board is unavailable. Use the text board below.
      </p>
    ) : (
      this.props.children
    );
  }
}

/** Load Phaser only on a board page; text controls remain available on failure. */
export function GameBoard(props: BoardProps) {
  const instructionsId = useId();
  if (!hasGameScene(props.gameId)) return null;
  return (
    <BoardBoundary key={props.gameId}>
      <Suspense fallback={<p role="status">Loading visual board…</p>}>
        <BoardHost {...props} instructionsId={instructionsId} />
      </Suspense>
      {!props.readOnly && (
        <p id={instructionsId} className="board-instructions">
          {
            (
              {
                tictactoe:
                  "Click a square, or use arrow keys and Enter. Highlighted squares are legal moves.",
                connect4:
                  "Click a column or press 1–7. Arrow keys and Enter also choose a column.",
                chess:
                  "Select a piece, then a highlighted destination. Use arrow keys and Enter, or click. Choose Q, R, B or N to promote; Escape clears selection.",
                sudoku:
                  "Select a cell, then enter a digit. Use N or Notes for pencil marks and Backspace or Erase to clear. Arrow keys move the selection.",
              } as Record<string, string>
            )[props.gameId]
          }{" "}
          Text and notation controls are below.
        </p>
      )}
    </BoardBoundary>
  );
}

/** Every engine is playable via its authoritative legal-action list. */
export function TextBoard({
  text,
  legalActions,
  onAction,
  disabled = false,
}: {
  text: string;
  legalActions: string[];
  onAction: (action: string) => void;
  disabled?: boolean;
}) {
  return (
    <section aria-label="Accessible game controls">
      <pre aria-live="polite">{text}</pre>
      <label>
        Legal move
        <select
          aria-label="Legal move"
          value=""
          disabled={disabled || legalActions.length === 0}
          onChange={(event) => onAction(event.target.value)}
        >
          <option value="">Choose a move</option>
          {legalActions.map((action) => (
            <option key={action} value={action}>
              {action}
            </option>
          ))}
        </select>
      </label>
    </section>
  );
}
