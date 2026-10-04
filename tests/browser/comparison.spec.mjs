import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";
const earlier = JSON.parse(await readFile(new URL("../../web/apps/site/src/report-example.json", import.meta.url), "utf8"));
const later = JSON.parse(await readFile(new URL("./fixtures/candidate-tournament.json", import.meta.url), "utf8"));
const failed = JSON.parse(await readFile(new URL("./fixtures/failed-tournament.json", import.meta.url), "utf8"));
const file = value => ({ name: value.run_id + ".json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(value)) });
const upload = (page, side, value) => page.getByLabel("Open run " + side + " report", { exact: true }).setInputFiles(file(value));

async function chooseSnapshots(page) {
  await upload(page, "A", earlier);
  await upload(page, "B", later);
  await page.getByLabel("Model in run A", { exact: true }).selectOption("example:policy-v1");
  await page.getByLabel("Model in run B", { exact: true }).selectOption("example:policy-v2");
}

test("compare native policy snapshots against shared opponents and switch models", async ({ page }) => {
  await page.goto("/#/reports");
  await page.getByRole("link", { name: "Compare runs", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Compare evaluations", exact: true })).toBeVisible();
  await chooseSnapshots(page);
  await expect(page.getByRole("status").filter({ hasText: "2 shared opponents" })).toBeVisible();
  const table = page.getByRole("region", { name: "Comparison by opponent", exact: true });
  const minimax = table.getByRole("row").filter({ hasText: "builtin:minimax:3" });
  await expect(minimax).toContainText("50.0%");
  await expect(minimax).toContainText("0.0%");
  await expect(minimax).toContainText("-50.0 pp");
  const random = table.getByRole("row").filter({ hasText: "builtin:random" });
  await expect(random).toContainText("0.0 pp");
  await page.getByLabel("Model in run B", { exact: true }).selectOption("builtin:random");
  await expect(page.getByRole("status").filter({ hasText: "1 shared opponent" })).toBeVisible();
  await expect(table).toContainText("No shared matchup");
});

test("different environments, position sets and schedules never produce a score change", async ({ page }) => {
  await page.goto("/#/reports/compare");
  await chooseSnapshots(page);
  const changed = structuredClone(later);
  changed.config = { max_moves: 10 };
  await upload(page, "B", changed);
  await expect(page.locator(".comparison-conditions")).toContainText("Settings differ or are incomplete: game options");
  await expect(page.locator(".comparison-delta")).toHaveCount(0);
  changed.config = {};
  delete changed.position_set;
  await upload(page, "B", changed);
  await expect(page.locator(".comparison-conditions")).toContainText("position-set provenance missing");
  await expect(page.locator(".comparison-delta")).toHaveCount(0);
  changed.position_set = { id: "other-starts", sha256: "a".repeat(64) };
  await upload(page, "B", changed);
  await expect(page.locator(".comparison-conditions")).toContainText("position set");
  await expect(page.locator(".comparison-delta")).toHaveCount(0);
  changed.position_set = null;
  changed.matches.forEach(match => { match.seed = String(BigInt(match.seed) + 1n); });
  await upload(page, "B", changed);
  await expect(page.getByRole("region", { name: "Comparison by opponent", exact: true })).toContainText("Schedules differ");
  await expect(page.locator(".comparison-delta")).toHaveCount(0);
});

test("failed games remain unrated and invalid replacement files preserve selected runs", async ({ page }) => {
  const writes = [];
  page.on("request", request => { if (request.method() !== "GET") writes.push(request.url()); });
  await page.goto("/#/reports/compare");
  await upload(page, "A", failed);
  await upload(page, "B", failed);
  await page.getByLabel("Model in run A", { exact: true }).selectOption("example:broken");
  await page.getByLabel("Model in run B", { exact: true }).selectOption("example:broken");
  const table = page.getByRole("region", { name: "Comparison by opponent", exact: true });
  await expect(table).toContainText("No rated games");
  await expect(table.getByText("2 excluded", { exact: true })).toHaveCount(2);
  await expect(page.locator(".comparison-delta")).toHaveCount(0);
  await upload(page, "B", { ...failed, format_version: 2 });
  await expect(page.getByRole("alert")).toContainText("format_version 1");
  await expect(page.getByLabel("Model in run B", { exact: true })).toHaveValue("example:broken");
  await expect(table).toContainText("2 excluded");
  expect(writes).toEqual([]);
});

test("comparison works by keyboard on mobile without overflowing the page", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto("/#/reports/compare");
  await chooseSnapshots(page);
  await page.getByRole("region", { name: "Comparison by opponent", exact: true }).focus();
  await expect(page.getByRole("region", { name: "Comparison by opponent", exact: true })).toBeFocused();
  await page.keyboard.press("ArrowRight");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("region", { name: "Run A", exact: true }).getByText("Run ID and game options", { exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "Run A", exact: true }).locator("details")).toHaveAttribute("open", "");
});
