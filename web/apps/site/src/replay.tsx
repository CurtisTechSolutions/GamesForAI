import { useCallback, useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { GameBoard } from "@gfa/game-kit";
import type { RecordedEvent } from "@gfa/api-client";
import { api } from "./api";
import { Failure, Loading } from "./feedback";
import { resultLabel } from "./result";
import { eventLabel } from "./match-events";
import { BranchForm, VariationTree } from "./variations";
import { EvaluationGraph } from "./replay-evaluation";

export function replayLink(id: string, turn = 0, view = "public") {
  return (
    "#/matches/" +
    encodeURIComponent(id) +
    "/replay?turn=" +
    turn +
    "&seat=" +
    view
  );
}
function eventTurn(event: RecordedEvent) {
  return event.type === "action"
    ? event.turn + 1
    : "turn" in event
      ? event.turn
      : 0;
}
export function ReplayPage({
  id,
  initialTurn = 0,
  initialSeat,
}: {
  id: string;
  initialTurn?: number;
  initialSeat?: number;
}) {
  const view = initialSeat === undefined ? "public" : String(initialSeat);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [copied, setCopied] = useState("");
  const [branchOpen, setBranchOpen] = useState(false);
  const replay = useQuery({
    queryKey: ["replay", id, initialSeat],
    queryFn: ({ signal }) => api.replay(id, initialSeat, signal),
    staleTime: 0,
  });
  const gameId = replay.data?.game_id;
  const game = useQuery({
    queryKey: ["game", gameId],
    queryFn: ({ signal }) => api.game(gameId!, signal),
    enabled: !!gameId,
  });
  const last = replay.data?.states.at(-1)?.turn ?? 0;
  const turn = Math.min(initialTurn, last);
  const go = useCallback(
    (next: number, perspective = view) => {
      window.history.replaceState(
        window.history.state,
        "",
        replayLink(id, next, perspective),
      );
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    },
    [id, view],
  );
  useEffect(() => {
    if (!playing || turn >= last) return;
    const timer = window.setTimeout(() => go(turn + 1), 1000 / speed);
    return () => window.clearTimeout(timer);
  }, [playing, speed, turn, last, go]);
  const frame = replay.data?.states.find((state) => state.turn === turn);
  if (replay.isError || game.isError)
    return (
      <Failure
        error={replay.error || game.error}
        retry={() => {
          void replay.refetch();
          if (game.isError) void game.refetch();
        }}
      />
    );
  if (replay.isPending || game.isPending)
    return <Loading label="Loading replay…" />;
  if (!frame || !game.data)
    return <p role="alert">This replay has no available state.</p>;
  const activePlayback = playing && turn < last;
  const selectTurn = (next: number) => {
    setPlaying(false);
    go(next);
  };
  return (
    <>
      <a className="back-link" href="#/history">
        ← Match history
      </a>
      <div className="match-heading">
        <div>
          <p className="eyebrow">Replay</p>
          <h1>{game.data.name}</h1>
        </div>
        <a
          className="button secondary"
          href={"#/matches/" + encodeURIComponent(id) + "/watch"}
        >
          Watch latest state
        </a>
      </div>
      <div className="match-layout">
        <section className="board-panel" aria-label="Replay board">
          <div className="section-heading">
            <h2>{resultLabel(frame, game.data.seat_names)}</h2>
            <output data-testid="replay-turn">
              Turn {turn} of {last}
            </output>
          </div>
          <GameBoard
            gameId={game.data.id}
            label={game.data.name}
            observation={frame.observation.json}
            perspective={initialSeat ?? 0}
            legalActions={[]}
            disabled
            readOnly
            onAction={() => undefined}
          />
          <div className="replay-controls">
            <label>
              Timeline
              <input
                type="range"
                aria-label="Replay turn"
                min={0}
                max={last}
                step={1}
                value={turn}
                onChange={(event) => selectTurn(Number(event.target.value))}
              />
            </label>
            <div className="playback-buttons">
              <button
                className="secondary"
                disabled={turn === 0}
                onClick={() => selectTurn(0)}
              >
                First
              </button>
              <button
                className="secondary"
                disabled={turn === 0}
                onClick={() => selectTurn(turn - 1)}
              >
                Previous
              </button>
              <button
                disabled={last === 0}
                onClick={() => {
                  if (!activePlayback && turn === last) go(0);
                  setPlaying(!activePlayback);
                }}
              >
                {activePlayback ? "Pause" : "Play"}
              </button>
              <button
                className="secondary"
                disabled={turn === last}
                onClick={() => selectTurn(turn + 1)}
              >
                Next
              </button>
              <button
                className="secondary"
                disabled={turn === last}
                onClick={() => selectTurn(last)}
              >
                Last
              </button>
            </div>
            <div className="form-grid">
              <label>
                Playback speed
                <select
                  aria-label="Playback speed"
                  value={speed}
                  onChange={(event) => setSpeed(Number(event.target.value))}
                >
                  {[0.5, 1, 2, 4].map((value) => (
                    <option key={value} value={value}>
                      {value}×
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Perspective
                <select
                  aria-label="Replay perspective"
                  value={view}
                  onChange={(event) => {
                    setPlaying(false);
                    go(turn, event.target.value);
                  }}
                >
                  <option value="public">Public spectator</option>
                  {game.data.seat_names.map((name, index) => (
                    <option value={index} key={name}>
                      {name} · seat {index}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </div>
          <details className="spectator-text">
            <summary>Text board</summary>
            <pre aria-live="polite">{frame.observation.text}</pre>
          </details>
          <div className="replay-links">
            <a href={replayLink(id, turn, view)}>Link to this turn</a>
            <button
              className="secondary"
              onClick={() => {
                void navigator.clipboard
                  .writeText(
                    new URL(replayLink(id, turn, view), window.location.href)
                      .href,
                  )
                  .then(
                    () => setCopied("Turn link copied."),
                    () => setCopied("Copy the turn link above."),
                  );
              }}
            >
              Copy turn link
            </button>
            <button
              className="secondary"
              disabled={replay.isFetching}
              onClick={() => void replay.refetch()}
            >
              Refresh replay
            </button>
          </div>
          {copied && <p role="status">{copied}</p>}
          <EvaluationGraph
            events={replay.data.events}
            names={game.data.seat_names}
            range={game.data.reward_range}
            last={last}
          />
          <button
            className="branch-button"
            onClick={() => {
              setPlaying(false);
              setBranchOpen(!branchOpen);
            }}
            aria-expanded={branchOpen}
          >
            Branch from here
          </button>
          {branchOpen && (
            <BranchForm
              key={turn}
              id={id}
              turn={turn}
              game={game.data}
              count={frame.returns.length}
            />
          )}
        </section>
        <aside className="match-sidebar">
          <section className="replay-moves" aria-label="Replay moves">
            <h2>Move list</h2>
            <ol>
              {replay.data.events.map((event) => (
                <li key={event.sequence}>
                  <button
                    className="replay-move"
                    aria-current={
                      turn === eventTurn(event) ? "step" : undefined
                    }
                    onClick={() => selectTurn(eventTurn(event))}
                  >
                    {eventLabel(event)}
                  </button>
                  {event.type === "action" && (
                    <>
                      {event.reasoning && (
                        <details>
                          <summary>Reasoning</summary>
                          <p>{event.reasoning}</p>
                        </details>
                      )}
                      {event.opponent_info && (
                        <details className="agent-record">
                          <summary>Agent details</summary>
                          <pre>
                            {JSON.stringify(event.opponent_info, null, 2)}
                          </pre>
                        </details>
                      )}
                    </>
                  )}
                </li>
              ))}
            </ol>
          </section>
          <VariationTree
            id={id}
            gameId={game.data.id}
            ancestors={replay.data.ancestors ?? []}
          />
          <details className="replay-record">
            <summary>Replay details</summary>
            <dl>
              <dt>Match</dt>
              <dd>{id}</dd>
              <dt>Engine version</dt>
              <dd>{replay.data.engine_version}</dd>
              <dt>Recorded events</dt>
              <dd>{replay.data.revision}</dd>
            </dl>
            <pre>{JSON.stringify(replay.data.config, null, 2)}</pre>
          </details>
        </aside>
      </div>
    </>
  );
}
