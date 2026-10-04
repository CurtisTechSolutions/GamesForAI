import { expect, test } from "@playwright/test";
const site = "http://127.0.0.1:18081";

async function open(page, request, game, extra = {}) {
  const response = await request.post("/v1/matches", { data: { game_id: game, seed: 2, include_info: false, ...extra } });
  expect(response.ok()).toBe(true);
  const { match_id: id } = await response.json();
  await page.goto(site + "/#/matches/" + id + "?seat=0&hotseat=1");
  const board = page.getByRole("group", { name: /interactive board/ });
  await expect(board).toHaveAttribute("data-ready", "true");
  await expect(page.getByLabel("Move notation")).toBeEnabled();
  return { id, board };
}
async function ready(page, turn) {
  await expect(page.locator(".turn-counter")).toHaveText("Turn " + turn);
  await expect(page.getByLabel("Move notation")).toBeEnabled();
}
async function cell(board, col, row, n) {
  const canvas = board.locator("canvas");
  const bounds = await canvas.boundingBox();
  await canvas.click({ position: { x: (40 + (col + .5) * 520 / n) * bounds.width / 600, y: (25 + (row + .5) * 520 / n) * bounds.height / 600 } });
}

test("Phaser loads on demand and Tic-Tac-Toe supports keyboard and legal-only clicks", async ({ page, request }) => {
  const loaded = [];
  page.on("request", request => loaded.push(request.url()));
  await page.goto(site);
  await expect(page.getByRole("heading", { name: /A playground/ })).toBeVisible();
  expect(loaded.some(url => /phaser\.js/.test(url))).toBe(false);
  const { id, board } = await open(page, request, "tictactoe");
  await board.focus();
  await board.press("Enter");
  await ready(page, 1);
  const submissions = [];
  page.on("request", request => { if (request.method() === "POST" && request.url().endsWith("/actions")) submissions.push(request); });
  await cell(board, 0, 0, 3);
  await board.press("ArrowDown");
  await board.press("Enter");
  await ready(page, 2);
  expect(submissions).toHaveLength(1);
  for (const [turn, keys] of [[3, ["ArrowUp", "ArrowRight"]], [4, ["ArrowDown"]], [5, ["ArrowUp", "ArrowRight"]]]) {
    for (const key of keys) await board.press(key);
    await board.press("Enter");
    if (turn < 5) await ready(page, turn);
  }
  await expect(page.getByTestId("match-status")).toHaveText("X wins");
  const final = await (await request.get(`/v1/matches/${id}/state?seat=0`)).json();
  expect(final.returns).toEqual([1, -1]);
});

test("Connect Four keyboard columns follow gravity through a full game", async ({ page, request }) => {
  const { id, board } = await open(page, request, "connect4");
  for (const [index, key] of ["1", "2", "1", "2", "1", "2", "1"].entries()) {
    await board.press(key);
    if (index < 6) await ready(page, index + 1);
  }
  await expect(page.getByTestId("match-status")).toHaveText("Red wins");
  const final = await (await request.get(`/v1/matches/${id}/state?seat=0`)).json();
  expect(final.observation.json.rows[5].slice(0, 2)).toEqual([0, 1]);
  expect(final.observation.json.rows[2][0]).toBe(0);
});

test("chess board uses server promotion choices", async ({ page, request }) => {
  const { id, board } = await open(page, request, "chess", { start: { position: "7k/P7/8/8/8/8/8/7K w - - 0 1" } });
  await cell(board, 0, 1, 8);
  await cell(board, 0, 0, 8);
  await expect(page.locator(".turn-counter")).toHaveText("Turn 0");
  await board.press("q");
  await ready(page, 1);
  const final = await (await request.get(`/v1/matches/${id}/state?seat=0`)).json();
  expect(final.observation.json.pieces.a8).toBe("Q");
});

test("mobile Sudoku protects givens and supports notes and touch digit entry", async ({ page, request }) => {
  await page.setViewportSize({ width: 360, height: 780 });
  const { id, board } = await open(page, request, "sudoku", { config: { size: 4 }, start: { position: "123434122143432." } });
  await cell(board, 0, 0, 4);
  await board.press("2");
  await expect(page.locator(".turn-counter")).toHaveText("Turn 0");
  await cell(board, 3, 3, 4);
  await board.press("n");
  await expect(page.getByRole("button", { name: "Notes", exact: true })).toHaveAttribute("aria-pressed", "true");
  await board.press("1");
  await ready(page, 1);
  const noted = await (await request.get(`/v1/matches/${id}/state?seat=0`)).json();
  expect(noted.observation.json.notes[15]).toBe(1);
  await page.getByRole("button", { name: "Notes", exact: true }).click();
  await page.getByRole("button", { name: "1", exact: true }).click();
  await expect(page.getByTestId("match-status")).toContainText("completed the puzzle");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  const bounds = await board.boundingBox();
  expect(bounds.width).toBeLessThanOrEqual(360);
});


test("renderer loading failures preserve playable text controls", async ({ page, request }) => {
  await page.route("**/game-kit/src/board-host.tsx*", route => route.abort());
  const response = await request.post("/v1/matches", { data: { game_id: "tictactoe", include_info: false } });
  const { match_id: id } = await response.json();
  await page.goto(site + "/#/matches/" + id + "?seat=0&hotseat=1");
  await expect(page.getByText("The visual board is unavailable. Use the text board below.")).toBeVisible();
  await page.getByLabel("Legal move", { exact: true }).selectOption("r2c2");
  await ready(page, 1);
});
