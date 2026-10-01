import { expect, test } from "@playwright/test";
import { ApiClient, ApiError } from "../../web/packages/api-client/src/index.ts";

const api = new ApiClient("http://127.0.0.1:18080");

test("typed browser client uses real catalog, briefing, play and replay contracts", async () => {
  const games = await api.games();
  expect(games.map((game) => game.id)).toContain("tictactoe");
  expect((await api.info("tictactoe")).sections.length).toBeGreaterThan(0);
  expect(await api.prompt("tictactoe")).toContain("Info version:");
  const opponents = await api.opponents("tictactoe");
  expect(opponents.map((entry) => entry.id)).toContain("random");
  const created = await api.create({ game_id: "tictactoe", seed: 51, include_info: false });
  const body = { seat: 0, turn: 0, action: "r2c2", reasoning: "browser-client-test" };
  const moved = await api.move(created.match_id, body, "client-test-first");
  expect(moved.state.turn).toBe(1);
  expect(await api.move(created.match_id, body, "client-test-first")).toEqual(moved);
  const state = await api.state(created.match_id, 1);
  expect(state.legal_actions).toHaveLength(8);
  const replay = await api.replay(created.match_id, 0);
  expect(replay.states).toHaveLength(2);
  expect(replay.events.some((entry) => JSON.stringify(entry).includes("browser-client-test"))).toBe(true);
  const events = await api.events(created.match_id, 0);
  expect(events.events.length).toBeGreaterThan(0);
  const first = await api.events(created.match_id, 0, undefined, undefined, 1);
  expect(first.events.map((event) => event.sequence)).toEqual([0]);
  expect(first.next).toBe(0);
  const second = await api.events(created.match_id, 0, first.next, undefined, 1);
  expect(second.events.map((event) => event.sequence)).toEqual([1]);
  let failure;
  try {
    await api.move(created.match_id, { seat: 0, turn: 0, action: "r1c1" }, "client-test-stale");
  } catch (error) { failure = error; }
  expect(failure).toBeInstanceOf(ApiError);
  expect(failure.status).toBe(409);
  expect(failure.code).toBe("STALE_TURN");
  expect(failure.hint.length).toBeGreaterThan(0);
  expect((await api.state(created.match_id, 1)).turn).toBe(1);
});

test("browser client rejects unsafe seeds, oversized input and supports cancellation", async () => {
  expect(() => api.create({ game_id: "tictactoe", seed: Number.MAX_SAFE_INTEGER + 1 })).toThrow("safe-integer");
  await expect(api.request("/outside")).rejects.toThrow("versioned API");
  await expect(api.request("/v1/matches", { method: "POST", body: "x".repeat(65537) })).rejects.toThrow("64 KiB");
  const controller = new AbortController();
  controller.abort();
  await expect(api.games(controller.signal)).rejects.toHaveProperty("name", "AbortError");
});
