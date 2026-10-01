import { useInfiniteQuery } from "@tanstack/react-query";
import type { RecordedEvent } from "@gfa/api-client";
import { api } from "./api";
import { Failure, Loading } from "./feedback";

export function eventLabel(event: RecordedEvent) {
  switch (event.type) {
    case "action": return "Turn " + (event.turn + 1) + " · seat " + event.seat + " · " + (event.action?.string ?? "Private move");
    case "created": return "Match created";
    case "forked_from": return "Branched at turn " + event.source.turn;
    case "resigned": return "Seat " + event.seat + " resigned";
    case "draw_offered": return "Seat " + event.seat + " offered or accepted a draw";
    case "finished": return event.truncated ? "Match stopped at the move limit" : "Match finished";
  }
}

export function MatchEvents({ id, seat }: { id: string; seat?: number }) {
  const events = useInfiniteQuery({
    queryKey: ["events", id, seat],
    initialPageParam: undefined as number | undefined,
    queryFn: ({ pageParam, signal }) => api.events(id, seat, pageParam, signal, 50),
    getNextPageParam: (page) => page.next ?? undefined,
  });
  return <section className="match-events" aria-label="Match events">
    <h2>Move list</h2>
    {events.isPending && <Loading label="Loading moves…" />}
    {events.isError && <Failure error={events.error} retry={() => void events.refetch()} />}
    <ol>{events.data?.pages.flatMap((page) => page.events).map((event) =>
      <li key={event.sequence}><span>{eventLabel(event)}</span>
        {event.type === "action" && event.reasoning && <details><summary>Reasoning</summary><p>{event.reasoning}</p></details>}
      </li>)}</ol>
    {events.hasNextPage && <button className="secondary" disabled={events.isFetchingNextPage} onClick={() => void events.fetchNextPage()}>Load more moves</button>}
  </section>;
}
