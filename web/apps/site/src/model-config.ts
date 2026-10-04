export type ModelConfig =
  | {
      id: string;
      type: "python";
      factory: string;
      params: Record<string, unknown>;
    }
  | {
      id: string;
      type: "chat";
      base_url: string;
      model: string;
      api_key_env?: string;
      timeout: number;
      max_tokens: number;
      temperature: number;
      structured: boolean;
    };

export interface ModelDraft {
  id: string;
  type: "python" | "chat";
  factory: string;
  params: string;
  baseUrl: string;
  model: string;
  keyEnv: string;
  timeout: string;
  maxTokens: string;
  temperature: string;
  structured: boolean;
}

export const emptyDraft: ModelDraft = {
  id: "",
  type: "python",
  factory: "my_agent:build_policy",
  params: "{}",
  baseUrl: "http://127.0.0.1:8000/v1",
  model: "",
  keyEnv: "",
  timeout: "60",
  maxTokens: "128",
  temperature: "0",
  structured: true,
};

function bytes(text: string) {
  return new TextEncoder().encode(text).length;
}
function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function number(
  text: string,
  name: string,
  min: number,
  max: number,
  integer = false,
) {
  const value = Number(text);
  if (
    !text.trim() ||
    !Number.isFinite(value) ||
    value < min ||
    value > max ||
    (integer && !Number.isInteger(value))
  )
    throw new Error(
      name +
        " must be " +
        (integer ? "a whole number " : "") +
        "between " +
        min +
        " and " +
        max +
        ".",
    );
  return value;
}
function finiteJson(value: unknown, depth = 0): boolean {
  if (depth > 24) return false;
  if (typeof value === "number") return Number.isFinite(value);
  if (Array.isArray(value))
    return value.every((item) => finiteJson(item, depth + 1));
  if (object(value))
    return Object.values(value).every((item) => finiteJson(item, depth + 1));
  return true;
}

export function modelFromDraft(draft: ModelDraft): ModelConfig {
  const id = draft.id.trim();
  if (!id || bytes(id) > 256)
    throw new Error("Give this snapshot a name of up to 256 bytes.");
  if (draft.type === "python") {
    const factory = draft.factory.trim();
    if (
      bytes(factory) > 256 ||
      !/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*:[A-Za-z_]\w*$/.test(factory)
    )
      throw new Error(
        "Use an importable Python factory, such as my_agent:build_policy.",
      );
    let params: unknown;
    try {
      params = JSON.parse(draft.params);
    } catch {
      throw new Error("Factory options must be valid JSON.");
    }
    if (!object(params) || !finiteJson(params) || bytes(draft.params) > 8192)
      throw new Error(
        "Factory options must be a finite JSON object within 8 KiB and 24 nesting levels.",
      );
    if ("seed" in params || "seat" in params)
      throw new Error(
        "Remove seed and seat from factory options. The runner supplies them for each game.",
      );
    return { id, type: "python", factory, params };
  }
  const baseUrl = draft.baseUrl.trim().replace(/\/+$/, "");
  let url: URL;
  try {
    url = new URL(baseUrl);
  } catch {
    throw new Error(
      "Enter the model server's full API root, such as http://127.0.0.1:8000/v1.",
    );
  }
  if (
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    baseUrl.length > 2048
  )
    throw new Error(
      "Use an HTTP or HTTPS API root without credentials, query parameters or a fragment.",
    );
  if (/\/chat\/completions$/i.test(url.pathname))
    throw new Error(
      "Use the API root ending in /v1, rather than /chat/completions.",
    );
  const model = draft.model.trim();
  if (!model || model.length > 256)
    throw new Error(
      "Enter the model identifier advertised by your server (up to 256 characters).",
    );
  const key = draft.keyEnv.trim();
  if (key && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(key))
    throw new Error(
      "Enter an environment variable name, such as MY_MODEL_KEY.",
    );
  const timeout = number(draft.timeout, "Timeout", 0.001, 300);
  return {
    id,
    type: "chat",
    base_url: baseUrl,
    model,
    ...(key ? { api_key_env: key } : {}),
    timeout,
    max_tokens: number(draft.maxTokens, "Output tokens", 1, 4096, true),
    temperature: number(draft.temperature, "Temperature", 0, 2),
    structured: draft.structured,
  };
}

export function draftFromModel(model: ModelConfig): ModelDraft {
  return model.type === "python"
    ? {
        ...emptyDraft,
        id: model.id,
        factory: model.factory,
        params: JSON.stringify(model.params, null, 2),
      }
    : {
        ...emptyDraft,
        id: model.id,
        type: "chat",
        baseUrl: model.base_url,
        model: model.model,
        keyEnv: model.api_key_env ?? "",
        timeout: String(model.timeout),
        maxTokens: String(model.max_tokens),
        temperature: String(model.temperature),
        structured: model.structured,
      };
}

const storageKey = "gamesforai.models.v1";
function storedModel(value: unknown): ModelConfig {
  if (!object(value) || typeof value.id !== "string")
    throw new Error("Invalid saved model.");
  const allowed =
    value.type === "python"
      ? ["id", "type", "factory", "params"]
      : [
          "id",
          "type",
          "base_url",
          "model",
          "api_key_env",
          "timeout",
          "max_tokens",
          "temperature",
          "structured",
        ];
  if (Object.keys(value).some((key) => !allowed.includes(key)))
    throw new Error("Invalid saved fields.");
  if (value.type === "python") {
    if (
      typeof value.factory !== "string" ||
      !object(value.params) ||
      !finiteJson(value.params)
    )
      throw new Error("Invalid saved factory.");
    return modelFromDraft({
      ...emptyDraft,
      id: value.id,
      factory: value.factory,
      params: JSON.stringify(value.params),
    });
  }
  if (
    value.type !== "chat" ||
    typeof value.base_url !== "string" ||
    typeof value.model !== "string" ||
    (value.api_key_env !== undefined &&
      typeof value.api_key_env !== "string") ||
    typeof value.timeout !== "number" ||
    typeof value.max_tokens !== "number" ||
    typeof value.temperature !== "number" ||
    typeof value.structured !== "boolean"
  )
    throw new Error("Invalid saved connection.");
  return modelFromDraft({
    ...emptyDraft,
    type: "chat",
    id: value.id,
    baseUrl: value.base_url,
    model: value.model,
    keyEnv: value.api_key_env ?? "",
    timeout: String(value.timeout),
    maxTokens: String(value.max_tokens),
    temperature: String(value.temperature),
    structured: value.structured,
  });
}

export function loadModels(): { models: ModelConfig[]; error: string } {
  try {
    const raw = localStorage.getItem(storageKey);
    if (!raw) return { models: [], error: "" };
    if (bytes(raw) > 131072) throw new Error("Saved models too large.");
    const value: unknown = JSON.parse(raw);
    if (
      !object(value) ||
      value.version !== 1 ||
      !Array.isArray(value.models) ||
      value.models.length > 12
    )
      throw new Error("Invalid saved collection.");
    const models = value.models.map(storedModel);
    if (new Set(models.map((model) => model.id)).size !== models.length)
      throw new Error("Duplicate saved names.");
    return { models, error: "" };
  } catch {
    return {
      models: [],
      error:
        "Saved configurations could not be read. You can still prepare and download a configuration below.",
    };
  }
}

export function saveModels(models: ModelConfig[]) {
  if (models.length > 12)
    throw new Error(
      "You can save up to 12 snapshots in this browser. Remove one to make room.",
    );
  const encoded = JSON.stringify({ version: 1, models });
  if (bytes(encoded) > 131072)
    throw new Error(
      "Saved configurations exceed 128 KiB. Reduce the factory options.",
    );
  try {
    localStorage.setItem(storageKey, encoded);
  } catch {
    throw new Error(
      "Browser storage is unavailable or full. Download your configuration to keep it.",
    );
  }
}

export interface EvaluationDraft {
  game: string;
  opponent: string;
  level: string;
  games: string;
  seed: string;
  trajectories: boolean;
  format: "jsonl" | "parquet";
  directory: string;
}

export function evaluationCommand(draft: EvaluationDraft) {
  if (!/^[a-z][a-z0-9_-]*$/.test(draft.game))
    throw new Error("Choose an installed two-player game.");
  const games = number(draft.games, "Games per opponent", 2, 1000, true);
  if (games % 2)
    throw new Error(
      "Choose an even number of games so both seats are represented equally.",
    );
  const seed = number(
    draft.seed,
    "Evaluation seed",
    0,
    Number.MAX_SAFE_INTEGER,
    true,
  );
  if (!["random", "minimax", "mcts"].includes(draft.opponent))
    throw new Error("Choose a supported opponent.");
  const opponent =
    draft.opponent === "random"
      ? "random"
      : draft.opponent +
        ":" +
        number(draft.level, "Opponent level", 1, 10, true);
  let command =
    "gfa tournament --game " +
    draft.game +
    " --agents agent.json --opponents " +
    opponent +
    " --games " +
    games +
    " --seed " +
    seed +
    " --report tournament-report.json";
  if (draft.trajectories) {
    if (!["jsonl", "parquet"].includes(draft.format))
      throw new Error("Choose JSONL or Parquet output.");
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(draft.directory))
      throw new Error(
        "Use a new output folder name with letters, numbers, hyphens or underscores.",
      );
    command +=
      " --trajectories " + draft.directory + " --format " + draft.format;
  }
  return command;
}

export function episodeScript(game: string, seed: number) {
  return [
    "from gamesforai import make",
    "from gamesforai.tournament_config import load_agent, load_document",
    "",
    'agent = load_agent(load_document("agent.json"), lambda: None)',
    "policy = agent.policy_factory(seed=" + seed + ", seat=0)",
    "env = make(" + JSON.stringify(game) + ', opponent="random")',
    "try:",
    "    observation, info = env.reset(seed=" + seed + ")",
    '    done = info["terminated"] or info["truncated"]',
    "    total_reward = 0.0",
    "    while not done:",
    "        action = policy(observation, info)",
    "        observation, reward, terminated, truncated, info = env.step(action)",
    "        total_reward += reward",
    "        done = terminated or truncated",
    '    print({"reward": total_reward, "terminated": info["terminated"], "truncated": info["truncated"]})',
    "finally:",
    "    env.close()",
    "",
  ].join("\n");
}
