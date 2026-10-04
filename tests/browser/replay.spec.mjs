import { expect, test } from "@playwright/test";
const site = "http://127.0.0.1:18081";
const examples = [
  ["tictactoe", {}, ["r1c1", "r2c1", "r1c2", "r2c2", "r1c3"]],
  ["connect4", {}, ["1", "2", "1", "2", "1", "2", "1"]],
  ["chess", {}, ["e2e4", "e7e5", "g1f3"]],
  ["sudoku", { config: { size: 4 }, start: { position: "123434122143432." } }, ["r4c4+1", "r4c4=1"]],
];
async function fixture(request, game, extra, moves) {
  const response = await request.post("/v1/matches", { data: { game_id: game, include_info: false, ...extra } });
  expect(response.ok()).toBe(true);
  let state = await response.json();
  const id = state.match_id;
  for (const action of moves) {
    const result = await request.post(`/v1/matches/${id}/actions`, { data: { seat: state.to_act[0], turn: state.turn, action, reasoning: "Recorded browser strategy at turn " + state.turn } });
    expect(result.ok()).toBe(true);
    state = (await result.json()).state;
  }
  return id;
}
for (const [game, extra, moves] of examples) test(`${game} replays the selected turn and branches without changing its parent`, async ({ page, request }) => {
  if (game === "sudoku") await page.setViewportSize({ width: 360, height: 800 });
  const id = await fixture(request, game, extra, moves);
  const original = await (await request.get(`/v1/matches/${id}/replay`)).json();
  await page.goto(site + `/#/matches/${id}/replay?turn=1`);
  await expect(page.getByTestId("replay-turn")).toHaveText(`Turn 1 of ${moves.length}`);
  await page.getByText("Text board", { exact: true }).click();
  await expect(page.locator(".spectator-text pre")).toHaveText(original.states[1].observation.text);
  await page.getByRole("button", { name: "Branch from here", exact: true }).click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("button", { name: "Branch and play", exact: true }).click();
  await expect(page.getByLabel("Legal move", { exact: true })).toBeEnabled();
  const child = /#\/matches\/([^/?]+)/.exec(page.url())[1];
  expect(child).not.toBe(id);
  await expect(page.locator(".turn-counter")).toHaveText("Turn 0");
  const metadata = await (await request.get(`/v1/matches/${child}`)).json();
  expect(metadata.forked_from).toEqual({ match_id: id, turn: 1 });
  await page.getByLabel("Legal move", { exact: true }).selectOption({ index: 1 });
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  expect(await (await request.get(`/v1/matches/${id}/replay`)).json()).toEqual(original);
  await page.getByRole("link", { name: "Replay and branch", exact: true }).click();
  await expect(page.getByRole("region", { name: "Variations", exact: true })).toContainText(id);
  await page.getByRole("link", { name: "Parent at turn 1", exact: true }).click();
  await expect(page.getByTestId("replay-turn")).toHaveText(`Turn 1 of ${moves.length}`);
  await expect(page.getByRole("link", { name: "Variation from turn 1", exact: true })).toBeVisible();
});

test("replay timeline, perspective, autoplay, reasoning and turn links stay in sync", async ({ page, request, context }) => {
  const id = await fixture(request, ...examples[0]);
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto(site + `/#/matches/${id}/replay?turn=2`);
  await expect(page.getByTestId("replay-turn")).toHaveText("Turn 2 of 5");
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.getByTestId("replay-turn")).toHaveText("Turn 3 of 5");
  await page.getByLabel("Replay turn").press("ArrowLeft");
  await expect(page.getByTestId("replay-turn")).toHaveText("Turn 2 of 5");
  await page.getByLabel("Replay perspective").selectOption("1");
  await expect(page.getByTestId("replay-turn")).toHaveText("Turn 2 of 5");
  await page.getByRole("button", { name: "Copy turn link", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Turn link copied." })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(site + `/#/matches/${id}/replay?turn=2&seat=1`);
  await page.getByText("Reasoning", { exact: true }).first().click();
  await expect(page.getByText("Recorded browser strategy at turn 0", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "First", exact: true }).click();
  await page.getByLabel("Playback speed").selectOption("4");
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(page.getByTestId("replay-turn")).toHaveText("Turn 5 of 5");
  await expect(page.getByRole("button", { name: "Play", exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByTestId("replay-turn")).toHaveText("Turn 5 of 5");
});

test("a replay can branch against an installed opponent and show recorded estimates", async ({ page, request }) => {
  const id = await fixture(request, ...examples[0]);
  await page.goto(site + `/#/matches/${id}/replay?turn=1`);
  await page.getByRole("button", { name: "Branch from here", exact: true }).click();
  await page.getByLabel("Variation players").selectOption("opponent");
  await page.getByLabel("Variation opponent").selectOption("minimax");
  await page.getByRole("button", { name: "Branch and play", exact: true }).click();
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  const child = /#\/matches\/([^/?]+)/.exec(page.url())[1];
  const state = await (await request.get(`/v1/matches/${child}/state?seat=0`)).json();
  await request.post(`/v1/matches/${child}/resign`, { data: { seat: 0, turn: state.turn } });
  await page.goto(site + `/#/matches/${child}/replay`);
  await expect(page.getByRole("img", { name: "Recorded engine estimates", exact: true })).toBeVisible();
  await page.getByText("Agent details", { exact: true }).click();
  await expect(page.locator(".agent-record pre")).toContainText("evaluation");
});
