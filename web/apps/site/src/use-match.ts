import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { MatchState } from "@gfa/api-client";
import { api } from "./api";

export function useMatch(id: string, seat: number | undefined) {
  const cache = useQueryClient();
  const [connection, setConnection] = useState("connecting");
  const metadata = useQuery({
    queryKey: ["match", id],
    queryFn: ({ signal }) => api.metadata(id, signal),
    refetchInterval: 10000,
  });
  const state = useQuery({
    queryKey: ["state", id, seat],
    queryFn: ({ signal }) => api.state(id, seat, signal),
    // A previously viewed seat may hold a snapshot from an earlier turn.
    // Refetch on perspective changes before hot-seat chooses the next player.
    staleTime: 0,
    structuralSharing: (previous, incoming) =>
      latest(previous as MatchState | undefined, incoming as MatchState),
    refetchInterval: 10000,
  });
  useEffect(
    () =>
      api.watch(
        id,
        seat,
        (incoming) => {
          cache.setQueryData<MatchState>(["state", id, seat], (current) =>
            latest(current, incoming),
          );
        },
        setConnection,
      ),
    [id, seat, cache],
  );
  const turn = state.data?.turn;
  const draw = state.data?.draw_offer;
  const ended = !!(state.data?.terminated || state.data?.truncated);
  useEffect(() => {
    void cache.invalidateQueries({ queryKey: ["match", id] });
    void cache.invalidateQueries({ queryKey: ["events", id] });
  }, [id, turn, draw, ended, cache]);
  const refresh = async () => {
    await Promise.all([
      cache.invalidateQueries({ queryKey: ["history"] }),
      cache.invalidateQueries({ queryKey: ["state", id] }),
      cache.invalidateQueries({ queryKey: ["match", id] }),
      cache.invalidateQueries({ queryKey: ["events", id] }),
    ]);
  };
  return { metadata, state, connection, refresh };
}

/** Controls can finish a match or offer a draw without advancing its turn. */
function latest(current: MatchState | undefined, incoming: MatchState) {
  if (!current) return incoming;
  if (current.turn > incoming.turn) return current;
  if (current.turn === incoming.turn) {
    if (
      (current.terminated || current.truncated) &&
      !incoming.terminated &&
      !incoming.truncated
    )
      return current;
    if (
      current.draw_offer != null &&
      incoming.draw_offer == null &&
      !incoming.terminated &&
      !incoming.truncated
    )
      return current;
  }
  return incoming;
}
