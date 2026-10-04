import { useEffect, useRef, useState } from "react";
import Phaser from "phaser";
import type { BoardProps } from "./types";
import { object, type BoardScene } from "./scene";
import { loadGameScene } from "./registry";

/** Phaser owns only pixels and input. React owns authoritative match data. */
export default function BoardHost(
  props: BoardProps & { instructionsId: string },
) {
  const host = useRef<HTMLDivElement>(null);
  const scene = useRef<BoardScene | null>(null);
  const latest = useRef(props);
  latest.current = props;
  const [failure, setFailure] = useState<Error>();
  const [ready, setReady] = useState(false);
  const [notes, setNotes] = useState(false);
  const size = Number(object(props.observation).size);
  const digits = [4, 6, 9].includes(size) ? size : 9;
  useEffect(() => {
    let stopped = false;
    let game: Phaser.Game | undefined;
    setReady(false);
    setNotes(false);
    void loadGameScene(props.gameId)
      .then(({ default: Scene }) => {
        if (stopped || !host.current) return;
        const board = new Scene();
        board.onReady = () => {
          if (stopped) return;
          scene.current = board;
          board.display(latest.current);
          board.events.on("board-notes", setNotes);
          board.events.on("board-action", (action: string) => {
            const current = latest.current;
            if (!current.disabled && current.legalActions.includes(action))
              current.onAction(action);
          });
          board.sys.game.canvas.setAttribute("aria-hidden", "true");
          setReady(true);
        };
        game = new Phaser.Game({
          type: Phaser.CANVAS,
          parent: host.current,
          width: 600,
          height: 600,
          backgroundColor: "#182731",
          input: { keyboard: false },
          scale: {
            mode: Phaser.Scale.FIT,
            autoCenter: Phaser.Scale.CENTER_BOTH,
          },
          scene: board,
          fps: { target: 30 },
          banner: false,
        });
      })
      .catch((error: unknown) => {
        if (!stopped)
          setFailure(
            error instanceof Error ? error : new Error("Board unavailable"),
          );
      });
    return () => {
      stopped = true;
      scene.current = null;
      game?.destroy(true);
    };
  }, [props.gameId]);
  useEffect(() => {
    scene.current?.display(props);
  }, [props]);
  if (failure) throw failure;
  return (
    <>
      <div
        ref={host}
        className="visual-board"
        role="group"
        tabIndex={0}
        aria-label={props.label + " interactive board"}
        aria-describedby={props.instructionsId}
        data-ready={ready}
        onPointerDown={() => host.current?.focus({ preventScroll: true })}
        onKeyDown={(event) => {
          if (
            /^(ArrowLeft|ArrowRight|ArrowUp|ArrowDown|Enter| |Escape|Backspace|Delete|[0-9nqrbNQRB])$/.test(
              event.key,
            )
          ) {
            event.preventDefault();
            scene.current?.events.emit("board-key", event.key);
          }
        }}
      />
      {props.gameId === "sudoku" && (
        <div className="board-keypad" role="group" aria-label="Sudoku keypad">
          {[
            ...Array.from({ length: digits }, (_, i) => String(i + 1)),
            "n",
            "Backspace",
          ].map((key) => (
            <button
              key={key}
              type="button"
              className="secondary"
              disabled={props.disabled}
              aria-pressed={key === "n" ? notes : undefined}
              onClick={() => scene.current?.events.emit("board-key", key)}
            >
              {key === "n" ? "Notes" : key === "Backspace" ? "Erase" : key}
            </button>
          ))}
        </div>
      )}
    </>
  );
}
