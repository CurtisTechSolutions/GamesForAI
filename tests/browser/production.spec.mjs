import { expect, test } from "@playwright/test";

test("server root loads the production app and plays through the same origin", async ({ page }) => {
  const failures = [];
  page.on("pageerror", (error) => failures.push(error.message));
  const response = await page.goto("/");
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toContain("text/html");
  await expect(page.locator(".game-card")).toHaveCount(4);
  await page.locator(".game-tictactoe.game-card").click();
  await page.getByRole("link", { name: "Play Tic-Tac-Toe", exact: true }).click();
  await page.getByRole("button", { name: "Start match", exact: true }).click();
  await expect(page.getByTestId("match-status")).toHaveText("X to move");
  await expect(page.getByText("Live connection", { exact: true })).toBeVisible();
  await page.getByLabel("Move notation").fill("r2c2");
  await page.getByRole("button", { name: "Play move", exact: true }).click();
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  await expect(page.getByTestId("match-status")).toHaveText("O to move");
  // Hash links remain reloadable when only the compiled bundle is served.
  await page.reload();
  await expect(page.locator(".turn-counter")).toHaveText("Turn 1");
  expect(failures).toEqual([]);
});

test("production assets have correct MIME types and do not replace missing API routes", async ({ request }) => {
  const index = await request.get("/");
  const html = await index.text();
  const assets = [...html.matchAll(/(?:src|href)="(\/assets\/[^\"]+)"/g)].map((match) => match[1]);
  expect(assets.length).toBeGreaterThan(0);
  for (const asset of assets) {
    const response = await request.get(asset);
    expect(response.status()).toBe(200);
    expect(response.headers()["content-type"]).toContain(asset.endsWith(".css") ? "text/css" : "javascript");
    expect((await request.head(asset)).status()).toBe(200);
  }
  const missing = await request.get("/v1/missing");
  expect(missing.status()).toBe(404);
  expect((await missing.json()).error.code).toBe("ROUTE_NOT_FOUND");
  expect((await request.get("/assets/missing.js")).status()).toBe(404);
  expect((await request.get("/.env")).status()).toBe(404);
  expect((await request.get("/", { headers: { Origin: "https://foreign.example" } })).status()).toBe(403);
});
