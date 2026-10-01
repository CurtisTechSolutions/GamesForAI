import { expect, test } from "@playwright/test";
const site = "http://127.0.0.1:18081";

async function start(page, game = "tictactoe") {
  await page.goto(site + "/#/games/" + game + "/play");
  await expect(page.getByRole("button", { name: "Start match" })).toBeEnabled();
}
async function notation(page, action, turn) {
  await expect(page.getByLabel("Move notation")).toBeEnabled();
  await page.getByLabel("Move notation").fill(action);
  await page.getByRole("button", { name: "Play move", exact: true }).click();
  await expect(page.locator(".turn-counter")).toHaveText("Turn " + turn);
}
function matchId(page) { return /#\/matches\/([^?]+)/.exec(page.url())[1]; }

test("hot-seat plays a full game by keyboard and rejects unlisted notation", async ({ page, request }) => {
  await start(page);
  await page.getByRole("button", { name: "Start match" }).click();
  await expect(page.getByTestId("match-status")).toHaveText("X to move");
  await page.getByLabel("Move notation").fill("r9c9");
  await page.getByRole("button", { name: "Play move", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Choose a legal move");
  await expect(page.locator(".turn-counter")).toHaveText("Turn 0");
  for (const [i, move] of ["r1c1", "r2c1", "r1c2", "r2c2", "r1c3"].entries()) await notation(page, move, i + 1);
  await expect(page.getByTestId("match-status")).toHaveText("X wins");
  await expect(page.getByLabel("Move notation")).toBeDisabled();
  const replay = await (await request.get("/v1/matches/" + matchId(page) + "/replay?seat=0")).json();
  expect(replay.states).toHaveLength(6);
  expect(replay.states.at(-1).returns).toEqual([1, -1]);
});

test("opponent replies and custom position validation use the real engine", async ({ page }) => {
  await start(page, "chess");
  await page.getByLabel("Players", { exact: true }).selectOption("opponent");
  await page.getByLabel("Opponent", { exact: true }).selectOption("random");
  await page.getByRole("button", { name: "Start match" }).click();
  await notation(page, "e2e4", 2);
  await expect(page.getByTestId("match-status")).toHaveText("White to move");
  await expect(page.getByText("Engine hints are off for this match.")).toBeVisible();
  await start(page);
  await page.getByText("Custom starting position and game options", { exact: true }).click();
  await page.getByLabel("Starting position", { exact: true }).fill("invalid");
  await page.getByRole("button", { name: "Validate position" }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await page.getByLabel("Starting position", { exact: true }).fill(JSON.stringify({ board: [0,0,null,1,1,null,null,null,null], to_move: 0 }));
  await page.getByRole("button", { name: "Validate position" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Position is valid." })).toBeVisible();
  await page.getByRole("button", { name: "Start match" }).click();
  await notation(page, "r1c3", 1);
  await expect(page.getByTestId("match-status")).toHaveText("X wins");
});

test("single-player custom Sudoku completes at mobile width", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await start(page, "sudoku");
  await page.getByText("Custom starting position and game options", { exact: true }).click();
  await page.getByLabel("Game options (JSON)").fill('{"size":4}');
  await page.getByLabel("Starting position", { exact: true }).fill("123434122143432.");
  await page.getByRole("button", { name: "Start match" }).click();
  await expect(page.getByLabel("Legal move", { exact: true })).toBeEnabled();
  const option = await page.getByLabel("Legal move", { exact: true }).locator("option").allTextContents();
  const winning = option.find((value) => value.includes("r4c4") && value.endsWith("1") && !value.includes("note"));
  expect(winning).toBeTruthy();
  await page.getByLabel("Legal move", { exact: true }).selectOption(winning);
  await expect(page.getByTestId("match-status")).toContainText("completed the puzzle");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("draw agreement propagates to both views and resignation requires an explicit choice", async ({ page, context, request }) => {
  const created = await (await request.post("/v1/matches", { data: { game_id: "tictactoe", seed: 1 } })).json();
  await page.goto(site + "/#/matches/" + created.match_id + "?seat=0");
  const other = await context.newPage();
  await other.goto(site + "/#/matches/" + created.match_id + "?seat=1");
  await page.getByRole("button", { name: "Offer draw", exact: true }).click();
  await expect(other.getByRole("button", { name: "Accept draw", exact: true })).toBeVisible();
  await other.getByRole("button", { name: "Accept draw", exact: true }).click();
  await expect(page.getByTestId("match-status")).toHaveText("Draw by agreement");
  await expect(other.getByTestId("match-status")).toHaveText("Draw by agreement");
  await start(page);
  await page.getByRole("button", { name: "Start match" }).click();
  await page.getByRole("button", { name: "Resign", exact: true }).click();
  await page.getByRole("button", { name: "Keep playing" }).click();
  await expect(page.getByTestId("match-status")).toHaveText("X to move");
  await page.getByRole("button", { name: "Resign", exact: true }).click();
  await page.getByRole("button", { name: "Confirm resign" }).click();
  await expect(page.getByTestId("match-status")).toHaveText("X resigned");
  await other.close();
});

test("hints are explicit, stale submissions recover, and duplicate delivery commits once", async ({ page, request }) => {
  await start(page);
  await page.getByLabel("Allow engine hints for this match").check();
  await page.getByRole("button", { name: "Start match" }).click();
  await page.getByRole("button", { name: "Show engine hint" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Suggested move:" })).toBeVisible();
  const id = matchId(page);
  await page.route("**/v1/matches/*/actions", async (route) => {
    const body = route.request().postDataJSON();
    const result = await request.post("/v1/matches/" + id + "/actions", {
      headers: { "Idempotency-Key": "external-before-stale" },
      data: { seat: body.seat, turn: body.turn, action: "r2c2" },
    });
    expect(result.ok()).toBe(true);
    await route.continue();
  }, { times: 1 });
  await page.getByLabel("Move notation").fill("r1c1");
  await page.getByRole("button", { name: "Play move", exact: true }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  const input = { seat: 1, turn: 1, action: "r1c1" };
  const first = await request.post("/v1/matches/" + id + "/actions", { headers: { "Idempotency-Key": "same-delivery" }, data: input });
  const second = await request.post("/v1/matches/" + id + "/actions", { headers: { "Idempotency-Key": "same-delivery" }, data: input });
  expect(await first.json()).toEqual(await second.json());
  await expect(page.locator(".turn-counter")).toHaveText("Turn 2");
});
