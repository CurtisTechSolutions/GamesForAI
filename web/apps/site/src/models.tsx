import { useMemo, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "./api";
import { Failure, Loading } from "./feedback";
import { CopyButton, downloadText } from "./export-controls";
import {
  draftFromModel,
  emptyDraft,
  episodeScript,
  evaluationCommand,
  loadModels,
  modelFromDraft,
  saveModels,
  type EvaluationDraft,
  type ModelConfig,
  type ModelDraft,
} from "./model-config";
import "./models.css";

const initialRun: EvaluationDraft = {
  game: "",
  opponent: "random",
  level: "3",
  games: "20",
  seed: "42",
  trajectories: false,
  format: "jsonl",
  directory: "training-run-001",
};
function message(error: unknown) {
  return error instanceof Error
    ? error.message
    : "Check the configuration and try again.";
}

export function ModelsPage() {
  const [saved, setSaved] = useState(loadModels);
  const [draft, setDraft] = useState<ModelDraft>({ ...emptyDraft });
  const [run, setRun] = useState<EvaluationDraft>({ ...initialRun });
  const [notice, setNotice] = useState("");
  const [saveError, setSaveError] = useState("");
  const [attempted, setAttempted] = useState(false);
  const catalog = useQuery({
    queryKey: ["games"],
    queryFn: ({ signal }) => api.games(signal),
  });
  const games =
    catalog.data?.filter(
      (game) =>
        game.num_players[0] === 2 &&
        game.num_players[1] === 2 &&
        game.turn_structure === "sequential",
    ) ?? [];
  const gameId =
    games.find((game) => game.id === run.game)?.id ??
    games.find((game) => game.id === "connect4")?.id ??
    games[0]?.id ??
    "";
  const game = games.find((game) => game.id === gameId);
  const prepared = useMemo(() => {
    try {
      return { config: modelFromDraft(draft), error: "" };
    } catch (error) {
      return { config: undefined, error: message(error) };
    }
  }, [draft]);
  let command = "",
    runError = "";
  try {
    command = evaluationCommand({ ...run, game: gameId });
  } catch (error) {
    runError = message(error);
  }
  const patch = (value: Partial<ModelDraft>) => {
    setDraft((current) => ({ ...current, ...value }));
    setSaveError("");
    setNotice("");
  };
  const persist = (models: ModelConfig[]) => {
    saveModels(models);
    setSaved({ models, error: "" });
  };
  const save = (event: FormEvent) => {
    event.preventDefault();
    setAttempted(true);
    setNotice("");
    setSaveError("");
    if (!prepared.config) return;
    try {
      const model = prepared.config;
      const existing = saved.models.some((item) => item.id === model.id);
      persist(
        existing
          ? saved.models.map((item) => (item.id === model.id ? model : item))
          : [...saved.models, model],
      );
      setNotice("Configuration saved in this browser.");
    } catch (error) {
      setSaveError(message(error));
    }
  };
  const encoded = prepared.config
    ? JSON.stringify(prepared.config, null, 2) + "\n"
    : "";
  const jsonl = run.format === "jsonl";
  return (
    <>
      <section className="game-heading models-heading">
        <div>
          <p className="eyebrow">Bring your own intelligence</p>
          <h1>Your models</h1>
          <p>
            Configure a policy. Play repeatable games. Keep the data for your
            next training run.
          </p>
        </div>
        <span className="model-count">
          {saved.models.length} saved configuration
          {saved.models.length === 1 ? "" : "s"}
        </span>
      </section>
      <div className="model-workspace">
        <aside className="model-shelf" aria-label="Saved model configurations">
          <div className="model-section-title">
            <h2>Snapshots</h2>
            <button
              className="secondary"
              type="button"
              onClick={() => {
                setDraft({ ...emptyDraft });
                setAttempted(false);
                setNotice("");
                setSaveError("");
                document.getElementById("model-name")?.focus();
              }}
            >
              New
            </button>
          </div>
          <p className="model-help">
            Use a new name when weights or prompts change so runs stay easy to
            compare.
          </p>
          {saved.error && (
            <p role="alert" className="input-error">
              {saved.error}
            </p>
          )}
          {!saved.models.length && (
            <div className="model-empty">
              <span aria-hidden="true">↗</span>
              <h3>Your first model starts here</h3>
              <p>
                Choose a Python policy or a model server, then save its
                configuration.
              </p>
            </div>
          )}
          {saved.models.map((model) => (
            <article
              className="saved-model"
              key={model.id}
              aria-label={model.id}
            >
              <span className="model-runtime">
                {model.type === "python" ? "Python policy" : "HTTP model"}
              </span>
              <h3>{model.id}</h3>
              <p>{model.type === "python" ? model.factory : model.model}</p>
              <div className="model-actions">
                <button
                  className="secondary"
                  type="button"
                  aria-label={"Load " + model.id}
                  onClick={() => {
                    setDraft(draftFromModel(model));
                    setAttempted(false);
                    setNotice("");
                    setSaveError("");
                    document.getElementById("model-name")?.focus();
                  }}
                >
                  Load
                </button>
                <button
                  className="text-button"
                  type="button"
                  aria-label={"Remove " + model.id}
                  onClick={() => {
                    try {
                      persist(
                        saved.models.filter((item) => item.id !== model.id),
                      );
                      setNotice("Saved configuration removed.");
                    } catch (error) {
                      setSaveError(message(error));
                    }
                  }}
                >
                  Remove
                </button>
              </div>
            </article>
          ))}
          <p className="model-help">
            Configurations are saved in this browser. Download a copy to use
            them in your Python environment.
          </p>
        </aside>
        <div className="model-main">
          <form className="model-panel" onSubmit={save} noValidate>
            <div className="model-step">
              <span aria-hidden="true">01</span>
              <div>
                <h2>Configure your model</h2>
                <p>Use the connection your model already supports.</p>
              </div>
            </div>
            <label htmlFor="model-name">Snapshot name</label>
            <input
              id="model-name"
              value={draft.id}
              maxLength={256}
              placeholder="connect4-policy-v1"
              onChange={(event) => patch({ id: event.target.value })}
            />
            <fieldset className="model-type-picker">
              <legend>Connection type</legend>
              <label>
                <input
                  type="radio"
                  name="model-type"
                  value="python"
                  checked={draft.type === "python"}
                  onChange={() => patch({ type: "python" })}
                />
                <span>
                  <strong>Python policy</strong>
                  <small>Load your weights and return an action.</small>
                </span>
              </label>
              <label>
                <input
                  type="radio"
                  name="model-type"
                  value="chat"
                  checked={draft.type === "chat"}
                  onChange={() => patch({ type: "chat" })}
                />
                <span>
                  <strong>HTTP model</strong>
                  <small>Use an OpenAI-compatible model server.</small>
                </span>
              </label>
            </fieldset>
            {draft.type === "python" ? (
              <div className="model-fields">
                <label>
                  Python factory
                  <input
                    aria-label="Python factory"
                    aria-describedby="factory-help"
                    value={draft.factory}
                    maxLength={256}
                    onChange={(event) => patch({ factory: event.target.value })}
                    spellCheck={false}
                  />
                  <small id="factory-help">
                    An importable module:callable. The runner supplies seed and
                    seat for each game.
                  </small>
                </label>
                <label>
                  Factory options (JSON)
                  <textarea
                    aria-label="Factory options (JSON)"
                    aria-describedby="params-help"
                    value={draft.params}
                    rows={4}
                    maxLength={8192}
                    onChange={(event) => patch({ params: event.target.value })}
                    spellCheck={false}
                  />
                  <small id="params-help">
                    Pass checkpoint paths and model options. Keep credentials in
                    environment variables.
                  </small>
                </label>
                <details>
                  <summary>Python policy contract</summary>
                  <p>
                    Your factory returns a callable that receives the
                    observation and info, then returns one legal integer action
                    index. The action mask and canonical move list are available
                    in info.
                  </p>
                  <pre className="model-code">
                    {
                      'def build_policy(*, seed, seat, **options):\n    model = load_your_model(**options)\n    def policy(observation, info):\n        return model.choose_action(observation, info["action_mask"])\n    return policy'
                    }
                  </pre>
                  <p className="model-help">
                    Replace load_your_model and choose_action with your model's
                    loading and inference functions.
                  </p>
                </details>
              </div>
            ) : (
              <div className="model-fields">
                <label>
                  API root
                  <input
                    aria-label="API root"
                    aria-describedby="api-root-help"
                    value={draft.baseUrl}
                    type="url"
                    maxLength={2048}
                    onChange={(event) => patch({ baseUrl: event.target.value })}
                    spellCheck={false}
                  />
                  <small id="api-root-help">
                    Usually ends in /v1. The runner adds /chat/completions.
                  </small>
                </label>
                <label>
                  Model identifier
                  <input
                    value={draft.model}
                    maxLength={256}
                    placeholder="your-model-name"
                    onChange={(event) => patch({ model: event.target.value })}
                  />
                </label>
                <label>
                  API key environment variable (optional)
                  <input
                    aria-label="API key environment variable (optional)"
                    aria-describedby="key-env-help"
                    value={draft.keyEnv}
                    maxLength={128}
                    placeholder="MY_MODEL_KEY"
                    onChange={(event) => patch({ keyEnv: event.target.value })}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <small id="key-env-help">
                    Enter the variable name. Its value is read on the machine
                    running your model evaluation.
                  </small>
                </label>
                <details>
                  <summary>Response settings</summary>
                  <div className="form-grid">
                    <label>
                      Output tokens
                      <input
                        type="number"
                        min="1"
                        max="4096"
                        value={draft.maxTokens}
                        onChange={(event) =>
                          patch({ maxTokens: event.target.value })
                        }
                      />
                    </label>
                    <label>
                      Timeout (seconds)
                      <input
                        type="number"
                        min="0.001"
                        max="300"
                        step="any"
                        value={draft.timeout}
                        onChange={(event) =>
                          patch({ timeout: event.target.value })
                        }
                      />
                    </label>
                    <label>
                      Temperature
                      <input
                        type="number"
                        min="0"
                        max="2"
                        step="0.1"
                        value={draft.temperature}
                        onChange={(event) =>
                          patch({ temperature: event.target.value })
                        }
                      />
                    </label>
                  </div>
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={draft.structured}
                      onChange={(event) =>
                        patch({ structured: event.target.checked })
                      }
                    />
                    Use structured output
                  </label>
                  <p className="model-help">
                    Enable JSON Schema when your server supports it. Every reply
                    must contain one legal move, for example {'{"action":"4"}'}{" "}
                    in Connect Four.
                  </p>
                </details>
              </div>
            )}
            {attempted && prepared.error && (
              <p className="input-error" role="alert">
                {prepared.error}
              </p>
            )}
            {saveError && (
              <p className="input-error" role="alert">
                {saveError}
              </p>
            )}
            <div className="model-actions">
              <button type="submit">Save configuration</button>
              <button
                className="secondary"
                type="button"
                disabled={!prepared.config}
                onClick={() => downloadText("agent.json", encoded)}
              >
                Download agent.json
              </button>
            </div>
            <p role="status">{notice}</p>
            {prepared.config && (
              <details className="configuration-preview">
                <summary>Configuration preview</summary>
                <pre className="model-code" data-testid="model-config">
                  {encoded}
                </pre>
                <CopyButton text={encoded} label="Copy configuration" />
              </details>
            )}
          </form>
          <section className="model-panel" aria-labelledby="evaluate-model">
            <div className="model-step">
              <span aria-hidden="true">02</span>
              <div>
                <h2 id="evaluate-model">Prepare an evaluation</h2>
                <p>
                  Compare one frozen snapshot against a repeatable opponent.
                </p>
              </div>
            </div>
            {catalog.isPending && <Loading label="Loading evaluation games…" />}
            {catalog.isError && (
              <Failure
                error={catalog.error}
                retry={() => void catalog.refetch()}
              />
            )}
            <div className="form-grid">
              <label>
                Evaluation game
                <select
                  aria-label="Evaluation game"
                  value={gameId}
                  disabled={!games.length}
                  onChange={(event) =>
                    setRun({
                      ...run,
                      game: event.target.value,
                      opponent: "random",
                    })
                  }
                >
                  {!games.length && (
                    <option value="">No compatible games available</option>
                  )}
                  {games.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Evaluation opponent
                <select
                  aria-label="Evaluation opponent"
                  value={run.opponent}
                  onChange={(event) =>
                    setRun({ ...run, opponent: event.target.value })
                  }
                >
                  <option value="random">Random legal moves</option>
                  {game?.information === "perfect" && !game.stochastic && (
                    <>
                      <option value="minimax">Minimax</option>
                      <option value="mcts">Monte Carlo tree search</option>
                    </>
                  )}
                </select>
              </label>
              {run.opponent !== "random" && (
                <label>
                  Opponent level
                  <select
                    aria-label="Opponent level"
                    aria-describedby="level-help"
                    value={run.level}
                    onChange={(event) =>
                      setRun({ ...run, level: event.target.value })
                    }
                  >
                    {Array.from({ length: 10 }, (_, index) => (
                      <option key={index} value={index + 1}>
                        Level {index + 1}
                      </option>
                    ))}
                  </select>
                  <small id="level-help">
                    Levels set search budgets; strength calibration is pending.
                  </small>
                </label>
              )}
              <label>
                Games per opponent
                <input
                  type="number"
                  min="2"
                  max="1000"
                  step="2"
                  aria-label="Games per opponent"
                  aria-describedby="games-help"
                  value={run.games}
                  onChange={(event) =>
                    setRun({ ...run, games: event.target.value })
                  }
                />
                <small id="games-help">
                  Even counts give each model both seats.
                </small>
              </label>
              <label>
                Evaluation seed
                <input
                  inputMode="numeric"
                  maxLength={16}
                  value={run.seed}
                  onChange={(event) =>
                    setRun({ ...run, seed: event.target.value })
                  }
                />
              </label>
            </div>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={run.trajectories}
                onChange={(event) =>
                  setRun({ ...run, trajectories: event.target.checked })
                }
              />
              Save training trajectories
            </label>
            {run.trajectories && (
              <div className="form-grid">
                <label>
                  Dataset format
                  <select
                    aria-label="Dataset format"
                    value={run.format}
                    onChange={(event) =>
                      setRun({
                        ...run,
                        format: event.target.value as "jsonl" | "parquet",
                      })
                    }
                  >
                    <option value="jsonl">
                      JSONL — text, moves and rewards
                    </option>
                    <option value="parquet">
                      Parquet — numeric training data
                    </option>
                  </select>
                </label>
                <label>
                  New output folder
                  <input
                    aria-label="New output folder"
                    aria-describedby="folder-help"
                    value={run.directory}
                    maxLength={64}
                    onChange={(event) =>
                      setRun({ ...run, directory: event.target.value })
                    }
                  />
                  <small id="folder-help">
                    Choose a new folder for each run to keep datasets separate.
                  </small>
                </label>
              </div>
            )}
            <p className="model-help">
              Evaluation supports sequential two-player games. These runs
              execute in your Python environment and write a report and any
              selected datasets to disk.
            </p>
            {gameId && runError && (
              <p className="input-error" role="alert">
                {runError}
              </p>
            )}
            {prepared.config && command ? (
              <div className="model-launch">
                <h3>Run in your Python environment</h3>
                <ol>
                  <li>
                    Install the SDK from your GamesForAI checkout:{" "}
                    <code>
                      {run.trajectories && !jsonl
                        ? "python -m pip install '.[tournaments,datasets]'"
                        : "python -m pip install '.[tournaments]'"}
                    </code>
                  </li>
                  <li>
                    Download agent.json into the working folder. Make your
                    Python factory importable or start your model server.
                  </li>
                  {prepared.config.type === "chat" &&
                    prepared.config.api_key_env && (
                      <li>
                        Set <code>{prepared.config.api_key_env}</code> in the
                        environment where you run the command.
                      </li>
                    )}
                  <li>
                    Run the evaluation command below. Results are saved to
                    tournament-report.json. Open it in{" "}
                    <a href="#/reports">Reports</a> to explore the results.
                  </li>
                </ol>
                <pre className="model-code" data-testid="evaluation-command">
                  {command}
                </pre>
                <CopyButton text={command} label="Copy evaluation command" />
                <details>
                  <summary>Try a single episode first</summary>
                  <p>
                    This script plays your selected model against random moves
                    from seat 0. Run it with <code>python try-model.py</code>{" "}
                    after downloading both files.
                  </p>
                  <pre className="model-code">
                    {episodeScript(gameId, Number(run.seed))}
                  </pre>
                  <button
                    type="button"
                    className="secondary"
                    onClick={() =>
                      downloadText(
                        "try-model.py",
                        episodeScript(gameId, Number(run.seed)),
                        "text/x-python",
                      )
                    }
                  >
                    Download episode script
                  </button>
                </details>
              </div>
            ) : (
              <p className="model-next">
                Complete the model configuration and choose valid evaluation
                settings to generate your run command.
              </p>
            )}
          </section>
        </div>
      </div>
    </>
  );
}
