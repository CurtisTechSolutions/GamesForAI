import { useQuery } from "@tanstack/react-query";
import { GameBoard } from "@gfa/game-kit";
import { api } from "./api";
import { Failure, Loading } from "./feedback";
import { MatchEvents } from "./match-events";
import { resultLabel } from "./result";
import { useMatch } from "./use-match";

/** Always requests the public spectator projection, including in a local workspace. */
export function Spectator({
  id,
  compact = false,
}: {
  id: string;
  compact?: boolean;
}) {
  const { metadata, state, connection, refresh } = useMatch(id, undefined);
  const gameId = metadata.data?.game_id;
  const game = useQuery({
    queryKey: ["game", gameId],
    queryFn: ({ signal }) => api.game(gameId!, signal),
    enabled: !!gameId,
  });
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
  if (!metadata.data || !state.data || !game.data)
    return <Loading label="Loading live match…" />;
  const current = state.data;
  return (
    <section
      className={compact ? "spectator-card" : "spectator-room"}
      aria-label={"Watching " + id}
    >
      {!compact && (
        <a className="back-link" href="#/live">
          ← Live arena
        </a>
      )}
      <div className="match-heading">
        <div>
          <p className="eyebrow">Spectating</p>
          {compact ? <h2>{game.data.name}</h2> : <h1>{game.data.name}</h1>}
        </div>
        <span className="connection" role="status">
          {connection === "live"
            ? "Live connection"
            : connection === "connecting"
              ? "Connecting…"
              : "Reconnecting… polling for updates"}
        </span>
      </div>
      <div className={compact ? "" : "match-layout"}>
        <div className={compact ? "" : "board-panel"}>
          <div className="section-heading">
            <h2 data-testid="spectator-status">
              {resultLabel(current, game.data.seat_names)}
            </h2>
            <span className="turn-counter">Turn {current.turn}</span>
          </div>
          <GameBoard
            gameId={gameId!}
            label={game.data.name}
            observation={current.observation.json}
            perspective={0}
            legalActions={[]}
            disabled
            readOnly
            onAction={() => undefined}
          />
          <details className="spectator-text">
            <summary>Text board</summary>
            <pre aria-live="polite">{current.observation.text}</pre>
          </details>
          {compact ? (
            <a
              className="button secondary"
              href={"#/matches/" + encodeURIComponent(id) + "/watch"}
            >
              Open spectator view
            </a>
          ) : (
            <a href={"#/matches/" + encodeURIComponent(id)}>
              Open match controls
            </a>
          )}
        </div>
        <aside className={compact ? "" : "match-sidebar"}>
          <MatchEvents id={id} />
          <p className="history-note">
            This view shows public observations and reasoning made available by
            the game.
          </p>
        </aside>
      </div>
    </section>
  );
}
