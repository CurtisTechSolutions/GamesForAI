import { expect, test } from "@playwright/test";

const site = "http://127.0.0.1:18081";

test("library uses the live catalog and guide, supports search and copying a prompt", async ({ page }) => {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto(site);
  await expect(page.getByRole("heading", { name: "A playground for intelligence." })).toBeVisible();
  await expect(page.locator(".game-card")).toHaveCount(4);
  await page.getByRole("searchbox", { name: "Find a game" }).fill("chess");
  await expect(page.locator(".game-card")).toHaveCount(1);
  await page.locator(".game-card").click();
  await expect(page.getByRole("heading", { name: "Chess", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Available opponents" })).toBeVisible();
  await expect(page.locator(".guide-section")).not.toHaveCount(0);
  await page.getByRole("button", { name: "Copy as prompt" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Prompt copied." })).toBeVisible();
  const clipboard = await page.evaluate(() => navigator.clipboard.readText());
  expect(clipboard).toContain("Info version:");
  expect(clipboard.toLowerCase()).toContain("chess");
  await page.getByRole("link", { name: "← Game library" }).click();
  await page.getByRole("searchbox", { name: "Find a game" }).fill("no-such-game");
  await expect(page.getByRole("status")).toContainText("No games match");
});

test("mobile library and guide fit at 360px and retain keyboard navigation", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  await page.goto(site);
  await expect(page.locator(".game-card")).toHaveCount(4);
  await page.keyboard.press("Tab");
  await expect(page.getByRole("link", { name: "Skip to content" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("main")).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: "test-results/library-mobile.png", fullPage: true });
  await page.locator(".game-tictactoe.game-card").click();
  await expect(page.locator(".guide-section")).not.toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.getByRole("button", { name: "Copy as prompt" }).focus();
  await expect(page.getByRole("button", { name: "Copy as prompt" })).toBeFocused();
});

test("library explains an unavailable server and recovers with retry", async ({ page }) => {
  await page.route("**/v1/games", (route) => route.abort());
  await page.goto(site);
  await expect(page.getByRole("alert")).toBeVisible();
  await page.unroute("**/v1/games");
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.locator(".game-card")).toHaveCount(4);
});

test("local development proxy accepts its browser origin and rejects unrelated origins", async ({ page, request }) => {
  const valid = await request.post(site + "/v1/matches", {
    headers: { Origin: site }, data: { game_id: "tictactoe", seed: 17 },
  });
  expect(valid.status()).toBe(201);
  const match = await valid.json();
  const rejected = await request.post(site + "/v1/matches", {
    headers: { Origin: "http://unrelated.example" }, data: { game_id: "tictactoe" },
  });
  expect(rejected.status()).toBe(403);
  await page.goto(site);
  const result = await page.evaluate(async (id) => new Promise((resolve) => {
    const socket = new WebSocket("ws://127.0.0.1:18081/v1/matches/" + id + "/stream?seat=0");
    const timer = setTimeout(() => { socket.close(); resolve("timeout"); }, 5000);
    socket.onmessage = (event) => { clearTimeout(timer); socket.close(); resolve(JSON.parse(event.data).type); };
    socket.onerror = () => { clearTimeout(timer); socket.close(); resolve("error"); };
  }), match.match_id);
  expect(result).toBe("state");
  // A different local page is still a foreign origin for this proxy.
  await page.goto("http://127.0.0.1:18080/docs/");
  const foreign = await page.evaluate(async (id) => new Promise((resolve) => {
    const socket = new WebSocket("ws://127.0.0.1:18081/v1/matches/" + id + "/stream?seat=0");
    const timer = setTimeout(() => { socket.close(); resolve("timeout"); }, 5000);
    socket.onopen = () => { clearTimeout(timer); socket.close(); resolve("open"); };
    socket.onerror = () => { clearTimeout(timer); socket.close(); resolve("rejected"); };
  }), match.match_id);
  expect(foreign).toBe("rejected");
});
