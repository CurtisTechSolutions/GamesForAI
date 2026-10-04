import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";
const example = JSON.parse(await readFile(new URL("../../web/apps/site/src/report-example.json", import.meta.url), "utf8"));
const failed = JSON.parse(await readFile(new URL("./fixtures/failed-tournament.json", import.meta.url), "utf8"));
const file = (value, name = "my-tournament.json") => ({ name, mimeType: "application/json", buffer: Buffer.from(JSON.stringify(value)) });

test("report example shows real standings, rating intervals and bounded game pages", async ({ page }) => {
  await page.goto("/#/reports");
  await page.getByRole("button", { name: "Explore an example", exact: true }).click();
  await expect(page.getByText("Example data · native test policies", { exact: true })).toBeVisible();
  await expect(page.getByRole("region", { name: "Standings table", exact: true })).toContainText("builtin:minimax:3");
  await expect(page.getByRole("img", { name: /rating.*interval/ })).toHaveCount(3);
  await expect(page.locator(".episode-result")).toHaveCount(25);
  await page.getByRole("button", { name: "Next games", exact: true }).click();
  await expect(page.locator(".episode-result")).toHaveCount(5);
  await expect(page.getByText("Page 2 of 2", { exact: true })).toBeVisible();
  await page.getByLabel("Filter model", { exact: true }).selectOption("example:policy-v1");
  await expect(page.getByRole("status").filter({ hasText: "20 matching games" })).toBeVisible();
  await expect(page.locator(".episode-result")).toHaveCount(20);
  await page.getByLabel("Filter outcome", { exact: true }).selectOption("failed");
  await expect(page.getByText("No games match these filters.", { exact: true })).toBeVisible();
});

test("uploaded reports stay local and expose failure details without invented ratings", async ({ page }) => {
  const writes = [];
  page.on("request", request => { if (request.method() !== "GET") writes.push(request.url()); });
  await page.goto("/#/reports");
  await page.getByLabel("Open JSON report", { exact: true }).setInputFiles(file(failed));
  await expect(page.getByText("Local tournament report", { exact: true })).toBeVisible();
  await expect(page.getByText("No rated games", { exact: true })).toHaveCount(2);
  await page.getByLabel("Filter outcome", { exact: true }).selectOption("failed");
  await expect(page.locator(".episode-result")).toHaveCount(2);
  await page.locator(".episode-result summary").first().click();
  await expect(page.locator(".episode-result[open]")).toContainText("policy_factory");
  await page.getByLabel("Search games", { exact: true }).fill(failed.matches[1].id);
  await expect(page.locator(".episode-result")).toHaveCount(1);
  await page.getByText("Run configuration and provenance", { exact: true }).click();
  await expect(page.locator(".report-provenance")).toContainText(failed.run_id);
  const pending = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download report", exact: true }).click();
  expect(JSON.parse(await readFile(await (await pending).path(), "utf8"))).toEqual(failed);
  expect(writes).toEqual([]);
});

test("bad, nonfinite and oversized reports leave a valid report available", async ({ page }) => {
  await page.goto("/#/reports");
  const upload = page.getByLabel("Open JSON report", { exact: true });
  await upload.setInputFiles(file(example));
  await expect(page.locator(".episode-result")).toHaveCount(25);
  await upload.setInputFiles(file({ ...example, format_version: 999 }));
  await expect(page.getByRole("alert")).toContainText("format_version 1");
  await expect(page.locator(".episode-result")).toHaveCount(25);
  const duplicate = structuredClone(example); duplicate.matches[1].id = duplicate.matches[0].id;
  await upload.setInputFiles(file(duplicate));
  await expect(page.getByRole("alert")).toContainText("not a valid GamesForAI tournament report");
  const inconsistent = structuredClone(example); inconsistent.matches[0].returns.reverse();
  await upload.setInputFiles(file(inconsistent));
  await expect(page.getByRole("alert")).toContainText("not a valid GamesForAI tournament report");
  const malformed = JSON.stringify(example).replace('"rating":', '"rating":1e999,"ignored_rating":');
  await upload.setInputFiles({ name: "invalid.json", mimeType: "application/json", buffer: Buffer.from(malformed) });
  await expect(page.getByRole("alert")).toContainText("not a valid GamesForAI tournament report");
  await upload.setInputFiles({ name: "too-big.json", mimeType: "application/json", buffer: Buffer.alloc(8 * 1024 * 1024 + 1, " ") });
  await expect(page.getByRole("alert")).toContainText("smaller than 8 MiB");
});

test("mobile reports remain within the viewport and treat model text as data", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  const hostile = structuredClone(failed);
  hostile.game = "__proto__";
  const original = hostile.agent_specs[0].id;
  const replacement = '<img src="https://untrusted.example/x" onerror="alert(1)">';
  hostile.agent_specs[0].id = replacement;
  hostile.standings.find(row => row.agent === original).agent = replacement;
  for (const row of hostile.matches) row.agents = row.agents.map(agent => agent === original ? replacement : agent);
  const external = [];
  page.on("request", request => { if (request.url().includes("untrusted.example")) external.push(request.url()); });
  await page.goto("/#/reports");
  await page.getByLabel("Open JSON report", { exact: true }).setInputFiles(file(hostile));
  await expect(page.getByRole("region", { name: "Standings table", exact: true })).toContainText(replacement);
  await expect(page.getByRole("heading", { name: "__proto__", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.locator(".episode-result summary").first().focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".episode-result[open]")).toHaveCount(1);
  expect(external).toEqual([]);
});
