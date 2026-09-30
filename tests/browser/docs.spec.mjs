import { expect, test } from "@playwright/test";

test("bundled API explorer creates a match and plays a move", async ({ page }) => {
  const external = [];
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/*", (route) => {
    if (new URL(route.request().url()).origin !== "http://127.0.0.1:18080") {
      external.push(route.request().url());
      return route.abort();
    }
    return route.continue();
  });
  await page.goto("/docs");
  await expect(page.getByRole("heading", { name: /GamesForAI local API/ })).toBeVisible();

  const create = page.locator("#operations-Matches-create");
  await create.locator(".opblock-summary").click();
  await create.locator("textarea.body-param__text").fill(JSON.stringify({
    game_id: "tictactoe",
    seed: 42,
    include_info: false,
  }));
  const createdResponse = page.waitForResponse((response) =>
    response.url().endsWith("/v1/matches") && response.request().method() === "POST");
  await create.getByRole("button", { name: "Execute", exact: true }).click();
  const created = await createdResponse;
  expect(created.status()).toBe(201);
  const initial = await created.json();
  expect(initial.turn).toBe(0);
  await expect(create.locator(".responses-inner")).toContainText(initial.match_id);

  const move = page.locator("#operations-Matches-make_move");
  await move.locator(".opblock-summary").click();
  await move.locator('tr[data-param-name="id"] input').fill(initial.match_id);
  await move.locator("textarea.body-param__text").fill(JSON.stringify({
    seat: 0, turn: 0, action: "r2c2",
  }));
  const movedResponse = page.waitForResponse((response) =>
    response.url().endsWith("/actions") && response.request().method() === "POST");
  await move.getByRole("button", { name: "Execute", exact: true }).click();
  const moved = await movedResponse;
  expect(moved.status()).toBe(200);
  const result = await moved.json();
  expect(result.state.turn).toBe(1);
  await expect(move.locator(".responses-inner")).toContainText("r2c2");
  expect(errors).toEqual([]);
  expect(external).toEqual([]);
});
