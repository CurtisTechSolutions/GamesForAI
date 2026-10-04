import { expect, test } from "@playwright/test";
const site = "http://127.0.0.1:18081";
async function create(request, game = "tictactoe") {
  const response = await request.post("/v1/matches", { data: { game_id: game, seed: 13, include_info: false } });
  expect(response.ok()).toBe(true);
  return (await response.json()).match_id;
}
async function find(page, id) {
  const row = page.getByRole("article", { name: id, exact: true });
  for (let i = 0; i < 100 && !await row.isVisible(); i++) {
    const more = page.getByRole("button", { name: "Load more matches", exact: true });
    if (!await more.isVisible()) break;
    await more.click();
    await expect(page.getByRole("button", { name: "Loading…", exact: true })).toHaveCount(0);
  }
  await expect(row).toBeVisible();
  return row;
}

test("history filters real matches and follows cursors through empty filtered pages", async ({ page, request }) => {
  const candidates = [await create(request, "connect4"), await create(request, "connect4"), await create(request, "connect4")];
  const id = candidates.sort().at(-1);
  await request.post(`/v1/matches/${id}/resign`, { data: { seat: 1, turn: 0 } });
  await page.route("**/v1/matches?**", async route => {
    const url = new URL(route.request().url()); url.searchParams.set("limit", "2");
    await route.continue({ url: url.toString() });
  });
  await page.goto(site + "/#/history");
  await page.getByLabel("Filter game").selectOption("connect4");
  await page.getByLabel("Filter status").selectOption("finished");
  await page.getByLabel("Filter result").selectOption("0");
  await page.getByLabel("Search loaded matches").fill(id);
  await expect(page.getByRole("status").filter({ hasText: "Loading matches…" })).toHaveCount(0);
  await expect(page.getByText("No matches found in this page. More matches are available below.", { exact: false })).toBeVisible();
  const row = await find(page, id);
  await expect(row).toContainText("returns 1, -1");
  await row.getByRole("link", { name: "Watch", exact: true }).click();
  await expect(page.getByTestId("spectator-status")).toHaveText("Yellow resigned");
});

test("spectator stream updates the board while withholding live private reasoning", async ({ page, request }) => {
  const id = await create(request);
  await page.goto(site + `/#/matches/${id}/watch`);
  await expect(page.getByRole("status").filter({ hasText: "Live connection" })).toBeVisible();
  await request.post(`/v1/matches/${id}/actions`, { data: { seat: 0, turn: 0, action: "r2c2", reasoning: "private live strategy" } });
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  await expect(page.getByTestId("spectator-status")).toHaveText("O to move");
  await expect(page.getByText("private live strategy")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Play move", exact: true })).toHaveCount(0);
  await page.getByText("Text board", { exact: true }).click();
  await expect(page.locator("pre")).toContainText("X");
});

test("live arena selects a match and remains usable at mobile width", async ({ page, request }) => {
  const id = await create(request);
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto(site + "/#/live");
  await page.getByRole("button", { name: "Clear arena" }).click();
  await page.getByLabel("Search loaded matches").fill(id);
  await expect(page.getByRole("status").filter({ hasText: "Loading matches…" })).toHaveCount(0);
  const row = await find(page, id);
  await row.getByRole("checkbox").check();
  await expect(page.getByRole("heading", { name: "Watching 1 of 4" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Watching " + id })).toBeVisible();
  await request.post(`/v1/matches/${id}/actions`, { data: { seat: 0, turn: 0, action: "r1c1" } });
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});


test("spectator polling recovers updates when the stream is unavailable", async ({ page, request }) => {
  const id = await create(request);
  await page.routeWebSocket("**/stream*", socket => socket.close());
  await page.goto(site + `/#/matches/${id}/watch`);
  await expect(page.locator(".turn-counter")).toHaveText("Turn 0");
  await request.post(`/v1/matches/${id}/actions`, { data: { seat: 0, turn: 0, action: "r1c1" } });
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1", { timeout: 15000 });
  await expect(page.getByRole("status").filter({ hasText: "polling for updates" })).toBeVisible();
});
