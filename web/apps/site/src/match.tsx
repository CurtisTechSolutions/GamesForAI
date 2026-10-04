import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { ApiError } from "@gfa/api-client";
import type { MatchState, Schemas } from "@gfa/api-client";
import { TextBoard } from "@gfa/game-kit";
import { api } from "./api";
import { Failure, Loading } from "./feedback";
import { MatchEvents } from "./match-events";
import { useMatch } from "./use-match";

type MoveIntent = { body: Schemas["MoveRequest"]; key: string };
type ControlIntent = {
  kind: "resign" | "offer-draw";
  body: Schemas["ControlRequest"];
  key: string;
};

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

export function MatchPage({
  id,
  initialSeat,
  hotseat,
}: {
  id: string;
  initialSeat: number;
  hotseat: boolean;
}) {
  const [seat, setSeat] = useState(initialSeat);
  const [notation, setNotation] = useState("");
  const [inputError, setInputError] = useState("");
  const [confirmResign, setConfirmResign] = useState(false);
  const lock = useRef(false);
  const { metadata, state, connection, refresh } = useMatch(id, seat);
  const gameId = metadata.data?.game_id;
  const game = useQuery({
    queryKey: ["game", gameId],
    queryFn: ({ signal }) => api.game(gameId!, signal),
    enabled: !!gameId,
  });
  const opponents = useQuery({
    queryKey: ["opponents", gameId],
    queryFn: ({ signal }) => api.opponents(gameId!, signal),
    enabled: !!gameId,
  });
  const move = useMutation({
    mutationFn: (intent: MoveIntent) => api.move(id, intent.body, intent.key),
    onSuccess: () => {
      setNotation("");
      setInputError("");
    },
    onSettled: async () => {
      await refresh();
      lock.current = false;
    },
  });
  const control = useMutation({
    mutationFn: (intent: ControlIntent) =>
      api.control(id, intent.kind, intent.body, intent.key),
    onSuccess: () => setConfirmResign(false),
    onSettled: async () => {
      await refresh();
      lock.current = false;
    },
  });
  const hint = useMutation({
    mutationFn: (body: Schemas["AnalysisRequest"]) => api.analyze(body, seat),
  });
  const current = state.data;
  const actor = current?.to_act[0];
  useEffect(() => {
    if (
      hotseat &&
      actor !== undefined &&
      metadata.data?.seats?.[actor]?.type !== "opponent"
    )
      setSeat(actor);
  }, [hotseat, actor, metadata.data?.seats]);
  useEffect(() => {
    setInputError("");
    setNotation("");
    setConfirmResign(false);
  }, [seat]);
  if (metadata.isError || state.isError || game.isError)
    return (
      <Failure
        error={metadata.error || state.error || game.error}
        retry={() => {
          void refresh();
          if (game.isError) void game.refetch();
        }}
      />
    );
  if (metadata.isPending || state.isPending || game.isPending || !current)
    return <Loading label="Loading match…" />;
  const ended = current.terminated || current.truncated;
  const botSeat = metadata.data.seats?.[seat]?.type === "opponent";
  const busy = move.isPending || control.isPending;
  const canAct = !ended && current.to_act.includes(seat) && !botSeat && !busy;
  const submit = (action: string) => {
    if (!canAct || lock.current) return;
    if (!current.legal_actions.some((entry) => entry.string === action)) {
      setInputError("Choose a legal move from the list for the current turn.");
      return;
    }
    lock.current = true;
    setInputError("");
    move.mutate({
      body: { seat, turn: current.turn, action },
      key: crypto.randomUUID(),
    });
  };
  const sendControl = (kind: ControlIntent["kind"]) => {
    if (ended || botSeat || busy || lock.current) return;
    lock.current = true;
    control.mutate({
      kind,
      body: { seat, turn: current.turn },
      key: crypto.randomUUID(),
    });
  };
  const engine =
    opponents.data?.find((entry) => entry.id === "reference") ??
    opponents.data?.find((entry) => entry.id === "minimax") ??
    opponents.data?.find((entry) => entry.id === "mcts");
  const requestedHint = hint.variables?.from;
  const hintIsCurrent =
    requestedHint &&
    "match_id" in requestedHint &&
    requestedHint.turn === current.turn &&
    requestedHint.seat === seat;
  return (
    <>
      <a
        className="back-link"
        href={"#/games/" + encodeURIComponent(metadata.data.game_id)}
      >
        ← {game.data.name} guide
      </a>
      <div className="match-heading">
        <div>
          <p className="eyebrow">{hotseat ? "Hot-seat" : "Match room"}</p>
          <h1>{game.data.name}</h1>
        </div>
        <span className="connection" role="status">
          {connection === "live"
            ? "Live connection"
            : connection === "reconnecting"
              ? "Reconnecting… polling for updates"
              : "Connecting…"}
        </span>
      </div>
      <div className="match-layout">
        <section className="board-panel" aria-label="Play board">
          <div className="section-heading">
            <h2 role="status" data-testid="match-status">
              {resultLabel(current, game.data.seat_names)}
            </h2>
            <span className="turn-counter">Turn {current.turn}</span>
          </div>
          <TextBoard
            text={current.observation.text}
            legalActions={current.legal_actions.map((action) => action.string)}
            onAction={submit}
            disabled={!canAct}
          />
          <form
            className="notation-form"
            onSubmit={(event: FormEvent) => {
              event.preventDefault();
              submit(notation.trim());
            }}
          >
            <label>
              Move notation
              <input
                autoComplete="off"
                value={notation}
                disabled={!canAct}
                onChange={(event) => setNotation(event.target.value)}
                placeholder={current.legal_actions[0]?.string ?? ""}
                maxLength={128}
              />
            </label>
            <button type="submit" disabled={!canAct || !notation.trim()}>
              {move.isPending ? "Submitting…" : "Play move"}
            </button>
          </form>
          {inputError && (
            <p role="alert" className="input-error">
              {inputError}
            </p>
          )}
          <p className="notation-help">{game.data.action_notation}</p>
          {move.isError && (
            <>
              <Failure error={move.error} />
              {(!(move.error instanceof ApiError) ||
                move.error.status >= 500) &&
                move.variables.body.turn === current.turn && (
                  <button
                    onClick={() => {
                      if (!lock.current) {
                        lock.current = true;
                        move.mutate(move.variables);
                      }
                    }}
                  >
                    Retry submission
                  </button>
                )}
            </>
          )}
          {control.isError && <Failure error={control.error} />}
          {!ended && (
            <div className="match-controls">
              <button
                className="secondary"
                disabled={botSeat || busy}
                onClick={() => setConfirmResign(true)}
              >
                Resign
              </button>
              {current.returns.length === 2 && (
                <button
                  className="secondary"
                  disabled={botSeat || busy || current.draw_offer === seat}
                  onClick={() => sendControl("offer-draw")}
                >
                  {current.draw_offer !== null &&
                  current.draw_offer !== undefined &&
                  current.draw_offer !== seat
                    ? "Accept draw"
                    : current.draw_offer === seat
                      ? "Draw offered"
                      : "Offer draw"}
                </button>
              )}
            </div>
          )}
          {confirmResign && (
            <div
              className="confirm-action"
              role="group"
              aria-label="Confirm resignation"
            >
              <p>
                End this match by resigning as {game.data.seat_names[seat]}?
              </p>
              <button disabled={busy} onClick={() => sendControl("resign")}>
                Confirm resign
              </button>
              <button
                className="secondary"
                onClick={() => setConfirmResign(false)}
              >
                Keep playing
              </button>
            </div>
          )}
          {ended && (
            <p role="status">
              Final returns:{" "}
              {current.returns
                .map(
                  (value, index) =>
                    (game.data.seat_names[index] ?? index) + " " + value,
                )
                .join(" · ")}
            </p>
          )}
          <div className="hint-panel">
            {metadata.data.assists.allow_analysis && engine ? (
              <>
                <button
                  className="secondary"
                  disabled={!canAct || hint.isPending}
                  onClick={() =>
                    hint.mutate({
                      game_id: metadata.data.game_id,
                      from: { match_id: id, seat, turn: current.turn },
                      opponent: {
                        id: engine.id,
                        ...(engine.levels.length
                          ? {
                              level:
                                engine.levels[
                                  Math.min(2, engine.levels.length - 1)
                                ].level,
                            }
                          : {}),
                      },
                    })
                  }
                >
                  {hint.isPending ? "Analyzing…" : "Show engine hint"}
                </button>
                {hint.isSuccess && hintIsCurrent && (
                  <p role="status">
                    Suggested move:{" "}
                    {hint.data.best_moves
                      .map((action) => action.string)
                      .join(", ")}
                    {hint.data.advice ? " · " + hint.data.advice.summary : ""}
                  </p>
                )}
                {hint.isError && hintIsCurrent && (
                  <Failure error={hint.error} />
                )}
              </>
            ) : (
              <p>Engine hints are off for this match.</p>
            )}
          </div>
        </section>
        <aside className="match-sidebar">
          <label>
            Viewing seat
            <select
              value={seat}
              disabled={hotseat || busy}
              onChange={(event) => setSeat(Number(event.target.value))}
            >
              {game.data.seat_names.map((name, index) => (
                <option key={name} value={index}>
                  {name} · seat {index}
                </option>
              ))}
            </select>
          </label>
          {botSeat && <p>A built-in opponent controls this seat.</p>}
          {!ended && !current.to_act.includes(seat) && (
            <p>Waiting for the other player.</p>
          )}
          <MatchEvents id={id} seat={seat} />
          <details>
            <summary>Connect your agent</summary>
            <p>
              Use this match identifier and the open seat through the local API.
            </p>
            <label>
              Match ID
              <input
                readOnly
                value={id}
                onFocus={(event) => event.currentTarget.select()}
              />
            </label>
            <a href="/docs/" target="_blank" rel="noreferrer">
              Open API reference ↗
            </a>
          </details>
        </aside>
      </div>
    </>
  );
}
