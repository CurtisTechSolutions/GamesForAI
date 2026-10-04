import { useState } from "react";
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import type { GameSpec, Schemas } from "@gfa/api-client";
import { api } from "./api";
import { Failure, Loading } from "./feedback";

export function BranchForm({
  id,
  turn,
  game,
  count,
}: {
  id: string;
  turn: number;
  game: GameSpec;
  count: number;
}) {
  const cache = useQueryClient();
  const [mode, setMode] = useState("hotseat");
  const [seat, setSeat] = useState(0);
  const [opponent, setOpponent] = useState("random");
  const [level, setLevel] = useState(1);
  const opponents = useQuery({
    queryKey: ["opponents", game.id],
    queryFn: ({ signal }) => api.opponents(game.id, signal),
  });
  const selected = opponents.data?.find((player) => player.id === opponent);
  const fork = useMutation({
    mutationFn: (seats: Schemas["Seat"][]) =>
      api.fork(id, { turn, seats, include_info: false }, seat),
    onSuccess: async (result) => {
      await Promise.all([
        cache.invalidateQueries({ queryKey: ["variations"] }),
        cache.invalidateQueries({ queryKey: ["history"] }),
      ]);
      window.location.hash =
        "/matches/" +
        encodeURIComponent(result.match_id) +
        "?seat=" +
        seat +
        (mode === "hotseat" && count > 1 ? "&hotseat=1" : "");
    },
  });
  return (
    <form
      className="branch-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (fork.isPending || (mode === "opponent" && !selected)) return;
        fork.mutate(
          Array.from({ length: count }, (_, index) =>
            mode === "hotseat" || index === seat
              ? { type: "human" }
              : mode === "external"
                ? { type: "open" }
                : {
                    type: "opponent",
                    opponent: {
                      id: selected!.id,
                      ...(selected!.levels.length ? { level } : {}),
                    },
                  },
          ),
        );
      }}
    >
      <fieldset disabled={fork.isPending}>
        <legend>Play a new variation from turn {turn}</legend>
        <p>
          The original match stays in your history. This variation keeps its
          game options and assistance policy.
        </p>
        {count > 1 && (
          <div className="form-grid">
            <label>
              Players
              <select
                aria-label="Variation players"
                value={mode}
                onChange={(event) => setMode(event.target.value)}
              >
                <option value="hotseat">Hot-seat on this device</option>
                <option value="opponent">You vs built-in opponent</option>
                <option value="external">You vs external agent</option>
              </select>
            </label>
            <label>
              Your seat
              <select
                aria-label="Variation seat"
                value={seat}
                onChange={(event) => setSeat(Number(event.target.value))}
              >
                {game.seat_names.slice(0, count).map((name, index) => (
                  <option key={name} value={index}>
                    {name} · seat {index}
                  </option>
                ))}
              </select>
            </label>
          </div>
        )}
        {mode === "opponent" && (
          <div className="form-grid">
            <label>
              Opponent
              <select
                aria-label="Variation opponent"
                value={opponent}
                disabled={!opponents.data}
                onChange={(event) => {
                  setOpponent(event.target.value);
                  setLevel(
                    opponents.data?.find(
                      (player) => player.id === event.target.value,
                    )?.levels[0]?.level ?? 1,
                  );
                }}
              >
                {opponents.data?.map((player) => (
                  <option key={player.id} value={player.id}>
                    {player.name}
                  </option>
                ))}
              </select>
            </label>
            {!!selected?.levels.length && (
              <label>
                Level
                <select
                  aria-label="Variation level"
                  value={level}
                  onChange={(event) => setLevel(Number(event.target.value))}
                >
                  {selected.levels.map((level) => (
                    <option key={level.level} value={level.level}>
                      {level.level}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {opponents.isError && (
              <Failure
                error={opponents.error}
                retry={() => void opponents.refetch()}
              />
            )}
          </div>
        )}
        {fork.isError && <Failure error={fork.error} />}
        <button
          type="submit"
          disabled={fork.isPending || (mode === "opponent" && !selected)}
        >
          {fork.isPending ? "Creating variation…" : "Branch and play"}
        </button>
      </fieldset>
    </form>
  );
}
export function VariationTree({
  id,
  gameId,
  ancestors,
}: {
  id: string;
  gameId: string;
  ancestors: Schemas["ReplayAncestor"][];
}) {
  const branches = useInfiniteQuery({
    queryKey: ["variations", id],
    staleTime: 0,
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam, signal }) =>
      api.history({ game_id: gameId, after: pageParam, limit: 50 }, signal),
    getNextPageParam: (page) => page.next ?? undefined,
  });
  const children =
    branches.data?.pages
      .flatMap((page) => page.matches)
      .filter((match) => match.forked_from?.match_id === id) ?? [];
  return (
    <section className="variation-tree" aria-label="Variations">
      <h2>Variations</h2>
      <ol>
        {ancestors.map((ancestor) => (
          <li key={ancestor.source.match_id}>
            <a
              href={
                "#/matches/" +
                encodeURIComponent(ancestor.source.match_id) +
                "/replay?turn=" +
                ancestor.source.turn
              }
            >
              Parent at turn {ancestor.source.turn}
            </a>
            <small>{ancestor.source.match_id}</small>
          </li>
        ))}
        <li>
          <strong>This match</strong>
          {children.length > 0 && (
            <ul>
              {children.map((child) => (
                <li key={child.match_id}>
                  <a
                    href={
                      "#/matches/" +
                      encodeURIComponent(child.match_id) +
                      "/replay"
                    }
                  >
                    Variation from turn {child.forked_from!.turn}
                  </a>
                  <small>{child.match_id}</small>
                </li>
              ))}
            </ul>
          )}
        </li>
      </ol>
      {branches.isPending && <Loading label="Loading variations…" />}
      {branches.isError && (
        <Failure error={branches.error} retry={() => void branches.refetch()} />
      )}
      {branches.hasNextPage && (
        <button
          className="secondary"
          disabled={branches.isFetchingNextPage}
          onClick={() => void branches.fetchNextPage()}
        >
          Find more variations
        </button>
      )}
    </section>
  );
}
