import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

async function download(page, button) {
  const pending = page.waitForEvent("download");
  await page.getByRole("button", { name: button, exact: true }).click();
  const file = await pending;
  const output = test.info().outputPath(file.suggestedFilename());
  await file.saveAs(output);
  return { name: file.suggestedFilename(), text: await readFile(output, "utf8") };
}
async function open(page) {
  await page.goto("/#/models");
  await expect(page.getByRole("heading", { name: "Your models", exact: true })).toBeVisible();
}

test("Python model configuration saves, reloads and exports a runnable evaluation", async ({ page }) => {
  await open(page);
  await page.getByLabel("Snapshot name", { exact: true }).fill("connect4-policy-v1");
  await page.getByLabel("Python factory", { exact: true }).fill("my_agent:build_policy");
  await page.getByLabel("Factory options (JSON)", { exact: true }).fill('{"checkpoint":"weights/v1.pt","device":"cpu"}');
  await page.getByRole("button", { name: "Save configuration", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Configuration saved in this browser." })).toBeVisible();
  const exported = await download(page, "Download agent.json");
  expect(exported.name).toBe("agent.json");
  expect(JSON.parse(exported.text)).toEqual({ id: "connect4-policy-v1", type: "python", factory: "my_agent:build_policy", params: { checkpoint: "weights/v1.pt", device: "cpu" } });
  await page.reload();
  await page.getByRole("button", { name: "Load connect4-policy-v1", exact: true }).click();
  await expect(page.getByLabel("Factory options (JSON)", { exact: true })).toHaveValue(/weights\/v1.pt/);
  await page.getByLabel("Evaluation game", { exact: true }).selectOption("tictactoe");
  await expect(page.getByLabel("Evaluation game", { exact: true }).locator("option")).toHaveCount(3);
  await page.getByLabel("Evaluation opponent", { exact: true }).selectOption("minimax");
  await page.getByLabel("Opponent level", { exact: true }).selectOption("3");
  await page.getByLabel("Games per opponent", { exact: true }).fill("20");
  await page.getByLabel("Save training trajectories", { exact: true }).check();
  await page.getByLabel("Dataset format", { exact: true }).selectOption("parquet");
  await expect(page.getByTestId("evaluation-command")).toHaveText("gfa tournament --game tictactoe --agents agent.json --opponents minimax:3 --games 20 --seed 42 --report tournament-report.json --trajectories training-run-001 --format parquet");
  await page.getByText("Try a single episode first", { exact: true }).click();
  const script = await download(page, "Download episode script");
  expect(script.name).toBe("try-model.py");
  expect(script.text).toContain('env = make("tictactoe", opponent="random")');
  expect(script.text).toContain('load_document("agent.json")');
  await page.getByRole("button", { name: "Remove connect4-policy-v1", exact: true }).click();
  await page.reload();
  await expect(page.getByRole("article", { name: "connect4-policy-v1", exact: true })).toHaveCount(0);
});

test("HTTP model exports environment-based credentials without contacting the endpoint", async ({ page }) => {
  const external = [];
  page.on("request", request => { if (new URL(request.url()).hostname === "model.example") external.push(request.url()); });
  await open(page);
  await page.getByLabel("Snapshot name", { exact: true }).fill("http-model-v2");
  await page.getByRole("radio", { name: /HTTP model/ }).check();
  await page.getByLabel("API root", { exact: true }).fill("https://model.example/v1/");
  await page.getByLabel("Model identifier", { exact: true }).fill("my-model");
  await page.getByLabel("API key environment variable (optional)", { exact: true }).fill("MY_MODEL_KEY");
  await page.getByText("Response settings", { exact: true }).click();
  await page.getByLabel("Use structured output", { exact: true }).uncheck();
  await page.getByLabel("Output tokens", { exact: true }).fill("512");
  await page.getByLabel("Temperature", { exact: true }).fill("0.3");
  const file = await download(page, "Download agent.json");
  expect(JSON.parse(file.text)).toEqual({ id: "http-model-v2", type: "chat", base_url: "https://model.example/v1", model: "my-model", api_key_env: "MY_MODEL_KEY", timeout: 60, max_tokens: 512, temperature: 0.3, structured: false });
  await page.getByRole("button", { name: "Save configuration", exact: true }).click();
  expect(external).toEqual([]);
  expect(await page.evaluate(() => localStorage.getItem("gamesforai.models.v1"))).not.toContain('"api_key":');
});

test("invalid settings are explained and cannot produce a run configuration", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Save configuration", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Give this snapshot a name");
  await page.getByLabel("Snapshot name", { exact: true }).fill("test-policy");
  await page.getByLabel("Factory options (JSON)", { exact: true }).fill('{"seed":99}');
  await page.getByRole("button", { name: "Save configuration", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Remove seed and seat");
  await expect(page.getByRole("button", { name: "Download agent.json", exact: true })).toBeDisabled();
  await page.getByRole("radio", { name: /HTTP model/ }).check();
  await page.getByLabel("API root", { exact: true }).fill("https://user:secret@model.example/v1");
  await page.getByRole("button", { name: "Save configuration", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("without credentials");
  await page.getByLabel("API root", { exact: true }).fill("http://localhost:8000/v1/chat/completions");
  await expect(page.getByRole("alert")).toContainText("Use the API root ending in /v1");
  await page.getByLabel("API root", { exact: true }).fill("http://localhost:8000/v1");
  await page.getByLabel("Model identifier", { exact: true }).fill("local-model");
  await page.getByLabel("Games per opponent", { exact: true }).fill("3");
  await expect(page.getByRole("alert")).toContainText("even number of games");
  await expect(page.getByTestId("evaluation-command")).toHaveCount(0);
});

test("mobile model setup supports keyboard, download and unavailable browser storage", async ({ page, context }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await context.addInitScript(() => {
    Storage.prototype.getItem = () => { throw new Error("blocked"); };
    Storage.prototype.setItem = () => { throw new Error("blocked"); };
  });
  await open(page);
  await page.getByRole("button", { name: "New", exact: true }).click();
  await expect(page.getByLabel("Snapshot name", { exact: true })).toBeFocused();
  await page.getByLabel("Snapshot name", { exact: true }).fill("mobile-policy");
  await page.getByRole("button", { name: "Save configuration", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "Browser storage is unavailable or full." })).toBeVisible();
  expect(JSON.parse((await download(page, "Download agent.json")).text).id).toBe("mobile-policy");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("the library links to model setup and clipboard failure leaves selectable config", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("link", { name: "Set up a model", exact: false }).click();
  await page.getByLabel("Snapshot name", { exact: true }).fill("clipboard-policy");
  await page.getByText("Configuration preview", { exact: true }).click();
  await page.evaluate(() => { Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true }); });
  await page.getByRole("button", { name: "Copy configuration", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Copy unavailable." })).toBeVisible();
  await expect(page.getByTestId("model-config")).toContainText("clipboard-policy");
});
