import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { MatchMetadata } from "@gfa/api-client";
import { api } from "./api";
import { Failure, Loading } from "./feedback";
import { Spectator } from "./spectate";

export function playerLabel(match: MatchMetadata) {
  return (
    match.seats
      ?.map((seat, index) =>
        seat.type === "opponent"
          ? seat.opponent.id +
            (seat.opponent.level ? " · level " + seat.opponent.level : "")
          : (seat.type === "open" ? "Open agent" : "Human") +
            " · seat " +
            index,
      )
      .join(" vs ") || "External players"
  );
}
export function HistoryPage({ live = false }: { live?: boolean }) {
  const [game, setGame] = useState("");
  const [status, setStatus] = useState("");
  const [search, setSearch] = useState("");
  const [result, setResult] = useState("");
  const [selected, setSelected] = useState<string[] | null>(null);
  const catalog = useQuery({
    queryKey: ["games"],
    queryFn: ({ signal }) => api.games(signal),
  });
  const history = useInfiniteQuery({
    queryKey: ["history", game, live ? "active" : status],
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam, signal }) =>
      api.history(
        {
          game_id: game || undefined,
          status: live ? "active" : status || undefined,
          after: pageParam,
          limit: 50,
        },
        signal,
      ),
    getNextPageParam: (page) => page.next ?? undefined,
    refetchInterval: live ? 5000 : false,
  });
  const all = history.data?.pages.flatMap((page) => page.matches) ?? [];
  const matches = all.filter(
    (match) =>
      (!search ||
        (match.match_id + " " + match.game_id + " " + playerLabel(match))
          .toLowerCase()
          .includes(search.toLowerCase())) &&
      (!result ||
        (match.status === "finished" &&
          (result === "draw"
            ? !match.truncated && match.returns.every((value) => value === 0)
            : result === "stopped"
              ? match.truncated
              : match.returns[Number(result)] > 0))),
  );
  const chosen = selected ?? matches.slice(0, 4).map((match) => match.match_id);
  return (
    <>
      <section className="game-heading">
        <div>
          <p className="eyebrow">Your workspace</p>
          <h1>{live ? "Live arena" : "Match history"}</h1>
          <p>
            {live
              ? "Watch up to four matches as they unfold."
              : "Find a match, return to play, or explore what happened."}
          </p>
        </div>
      </section>
      <div className="history-filters">
        <label>
          Game
          <select
            aria-label="Filter game"
            value={game}
            onChange={(event) => {
              setGame(event.target.value);
              setSelected(null);
            }}
          >
            <option value="">All games</option>
            {catalog.data?.map((game) => (
              <option key={game.id} value={game.id}>
                {game.name}
              </option>
            ))}
          </select>
        </label>
        {!live && (
          <label>
            Status
            <select
              aria-label="Filter status"
              value={status}
              onChange={(event) => setStatus(event.target.value)}
            >
              <option value="">All matches</option>
              <option value="active">Active</option>
              <option value="finished">Finished</option>
            </select>
          </label>
        )}
        <label>
          Search loaded matches
          <input
            aria-label="Search loaded matches"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Match ID or player"
          />
        </label>
        {!live && (
          <label>
            Result
            <select
              aria-label="Filter result"
              value={result}
              onChange={(event) => setResult(event.target.value)}
            >
              <option value="">All results</option>
              <option value="0">Seat 0 won / puzzle solved</option>
              <option value="1">Seat 1 won</option>
              <option value="draw">Draw / no positive return</option>
              <option value="stopped">Move limit reached</option>
            </select>
          </label>
        )}
        <button
          className="secondary"
          disabled={history.isFetching}
          onClick={() => void history.refetch()}
        >
          Refresh matches
        </button>
      </div>
      {catalog.isError && (
        <Failure error={catalog.error} retry={() => void catalog.refetch()} />
      )}
      {history.isPending && <Loading label="Loading matches…" />}
      {history.isError && (
        <Failure error={history.error} retry={() => void history.refetch()} />
      )}
      {live && (
        <>
          <div className="section-heading">
            <h2>Watching {chosen.length} of 4</h2>
            <button className="secondary" onClick={() => setSelected([])}>
              Clear arena
            </button>
          </div>
          <div className="arena-grid">
            {chosen.map((id) => (
              <Spectator key={id} id={id} compact />
            ))}
          </div>
        </>
      )}
      <p className="history-note">
        {all.length} matches loaded. Search and result filters apply to loaded
        matches.{history.hasNextPage ? " Load more to continue searching." : ""}
      </p>
      {!history.isPending && !history.isError && matches.length === 0 && (
        <p role="status">
          No matches found
          {history.hasNextPage
            ? " in this page. More matches are available below."
            : "."}{" "}
          <a href="#/">Choose a game to start one.</a>
        </p>
      )}
      <div className="history-list">
        {matches.map((match) => (
          <article
            className="history-row"
            key={match.match_id}
            aria-label={match.match_id}
          >
            <div>
              <h2>
                {catalog.data?.find((game) => game.id === match.game_id)
                  ?.name ?? match.game_id}
              </h2>
              <p>{playerLabel(match)}</p>
              <small>
                {new Date(match.created_at_ms).toLocaleString()} ·{" "}
                {match.status} · turn {match.turn}
                {match.status === "finished"
                  ? " · returns " + match.returns.join(", ")
                  : ""}
              </small>
              <code>{match.match_id}</code>
              {match.forked_from && (
                <p className="history-note">
                  Variation from{" "}
                  <a
                    href={
                      "#/matches/" +
                      encodeURIComponent(match.forked_from.match_id) +
                      "/watch"
                    }
                  >
                    {match.forked_from.match_id}
                  </a>
                  , turn {match.forked_from.turn}
                </p>
              )}
            </div>
            <div className="history-actions">
              <a
                className="button secondary"
                href={
                  "#/matches/" + encodeURIComponent(match.match_id) + "/watch"
                }
              >
                Watch
              </a>
              <a
                className="button secondary"
                href={"#/matches/" + encodeURIComponent(match.match_id)}
              >
                Open match
              </a>
              {live && (
                <label className="checkbox-label">
                  <input
                    type="checkbox"
                    aria-label={"Watch " + match.match_id + " in arena"}
                    checked={chosen.includes(match.match_id)}
                    disabled={
                      !chosen.includes(match.match_id) && chosen.length >= 4
                    }
                    onChange={(event) =>
                      setSelected(
                        event.target.checked
                          ? [...chosen, match.match_id]
                          : chosen.filter((id) => id !== match.match_id),
                      )
                    }
                  />
                  In arena
                </label>
              )}
            </div>
          </article>
        ))}
      </div>
      {history.hasNextPage && (
        <button
          className="load-more"
          disabled={history.isFetchingNextPage}
          onClick={() => void history.fetchNextPage()}
        >
          {history.isFetchingNextPage ? "Loading…" : "Load more matches"}
        </button>
      )}
    </>
  );
}
