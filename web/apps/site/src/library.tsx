import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { GameSpec } from "@gfa/api-client";
import { api } from "./api";
import { Failure, Loading } from "./feedback";

const icons: Record<string, string> = {
  tictactoe: "× ○",
  connect4: "●",
  chess: "♞",
  sudoku: "1 9",
};

function GameCard({ game }: { game: GameSpec }) {
  const players =
    game.num_players[0] === game.num_players[1]
      ? String(game.num_players[0])
      : game.num_players.join("–");
  return (
    <a
      className={`game-card game-${game.id}`}
      href={`#/games/${encodeURIComponent(game.id)}`}
    >
      <div className="card-art" aria-hidden="true">
        <div className="mini-grid">
          {Array.from({ length: 16 }, (_, index) => (
            <span key={index}>
              {game.id === "connect4"
                ? "●"
                : game.id === "chess"
                  ? index === 5
                    ? "♞"
                    : ""
                  : game.id === "sudoku"
                    ? [1, 7, 10, 14].includes(index)
                      ? String((index % 9) + 1)
                      : ""
                    : index === 5
                      ? "×"
                      : index === 10
                        ? "○"
                        : ""}
            </span>
          ))}
        </div>
        <span className="card-glyph">{icons[game.id] || "◇"}</span>
      </div>
      <div className="card-copy">
        <div className="card-meta">
          <span>
            {players} {players === "1" ? "player" : "players"}
          </span>
          <span>
            {game.information === "perfect"
              ? "Open information"
              : "Hidden information"}
          </span>
        </div>
        <h2>
          {game.name}
          <span aria-hidden="true">↗</span>
        </h2>
        <p>{game.summary}</p>
        <span className="card-link">
          Explore game <span aria-hidden="true">→</span>
        </span>
      </div>
    </a>
  );
}

export function Library() {
  const games = useQuery({
    queryKey: ["games"],
    queryFn: ({ signal }) => api.games(signal),
  });
  const [filter, setFilter] = useState("");
  const visible = games.data?.filter((game) =>
    `${game.name} ${game.summary}`.toLowerCase().includes(filter.toLowerCase()),
  );
  return (
    <>
      <section className="library-intro">
        <p className="eyebrow">
          <span className="status-dot" />
          Play. Train. Evaluate.
        </p>
        <h1>
          A playground
          <br />
          for intelligence.
        </h1>
        <p>
          Explore the rules, test a strategy, and give your models room to
          learn.
        </p>
      </section>
      <div className="section-heading">
        <div>
          <h2>Game library</h2>
          <span className="muted">
            {games.data
              ? `${games.data.length} installed environments`
              : "Your game environments"}
          </span>
        </div>
        <label className="search">
          <span>Find a game</span>
          <input
            type="search"
            placeholder="Search games"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
        </label>
      </div>
      {games.isPending && <Loading />}
      {games.isError && (
        <Failure error={games.error} retry={() => void games.refetch()} />
      )}
      {games.data && (
        <div className="game-grid">
          {visible?.map((game) => (
            <GameCard key={game.id} game={game} />
          ))}
        </div>
      )}
      {visible?.length === 0 && (
        <p role="status" className="feedback">
          No games match “{filter}”. Try a different name.
        </p>
      )}
      <aside className="training-callout">
        <div>
          <p className="eyebrow">Bring your own model</p>
          <h2>One set of rules. Your choice of intelligence.</h2>
          <p>
            Connect a Python policy or your model server, then train and
            evaluate with the same game environments.
          </p>
        </div>
        <a className="button secondary" href="#/models">
          Set up a model <span aria-hidden="true">→</span>
        </a>
      </aside>
    </>
  );
}

function sectionTitle(id: string) {
  return id
    .replaceAll("_", " ")
    .replace(/^./, (letter) => letter.toUpperCase());
}

export function GameDetails({ id }: { id: string }) {
  const game = useQuery({
    queryKey: ["game", id],
    queryFn: ({ signal }) => api.game(id, signal),
  });
  const info = useQuery({
    queryKey: ["info", id],
    queryFn: ({ signal }) => api.info(id, signal),
  });
  const opponents = useQuery({
    queryKey: ["opponents", id],
    queryFn: ({ signal }) => api.opponents(id, signal),
  });
  const [prompt, setPrompt] = useState("");
  const [copyStatus, setCopyStatus] = useState("");
  const copy = useMutation({
    mutationFn: () => api.prompt(id),
    onSuccess: async (text) => {
      setPrompt(text);
      try {
        await navigator.clipboard.writeText(text);
        setCopyStatus("Prompt copied.");
      } catch {
        setCopyStatus("Prompt ready. Select and copy the text below.");
      }
    },
  });
  if (game.isPending) return <Loading label="Loading game…" />;
  if (game.isError)
    return <Failure error={game.error} retry={() => void game.refetch()} />;
  return (
    <>
      <a className="back-link" href="#/">
        ← Game library
      </a>
      <section className="game-heading">
        <div>
          <p className="eyebrow">Explore the environment</p>
          <h1>{game.data.name}</h1>
          <p>{game.data.summary}</p>
        </div>
        <span className={`detail-glyph game-${id}`} aria-hidden="true">
          {icons[id] || "◇"}
        </span>
      </section>
      <a
        className="button"
        href={"#/games/" + encodeURIComponent(id) + "/play"}
      >
        Play {game.data.name}
      </a>
      <div className="facts">
        <span>{game.data.num_players.join("–")} players</span>
        <span>
          {game.data.information === "perfect"
            ? "Open information"
            : "Hidden information"}
        </span>
        <span>
          {game.data.stochastic ? "Seeded chance" : "Deterministic rules"}
        </span>
        <span>Engine {game.data.engine_version}</span>
      </div>
      <div className="detail-grid">
        <section className="rulebook">
          <div className="section-heading">
            <h2>Rules &amp; play guide</h2>
            <button onClick={() => copy.mutate()} disabled={copy.isPending}>
              {copy.isPending ? "Preparing…" : "Copy as prompt"}
            </button>
          </div>
          <p role="status" className="copy-status">
            {copyStatus}
          </p>
          {copy.isError && <Failure error={copy.error} />}
          {prompt && (
            <details>
              <summary>View model prompt</summary>
              <label className="prompt-label">
                Game prompt
                <textarea
                  readOnly
                  value={prompt}
                  onFocus={(event) => event.currentTarget.select()}
                  rows={10}
                />
              </label>
            </details>
          )}
          {info.isPending && <Loading label="Loading play guide…" />}
          {info.isError && (
            <Failure error={info.error} retry={() => void info.refetch()} />
          )}
          {info.data?.sections.map((section, index) => (
            <details
              className="guide-section"
              key={section.id}
              open={index < 2 || section.id === "rules"}
            >
              <summary>{sectionTitle(section.id)}</summary>
              <div className="guide-text">{section.text}</div>
              {section.data !== null && (
                <details className="structured-facts">
                  <summary>Structured details</summary>
                  <pre>{JSON.stringify(section.data, null, 2)}</pre>
                </details>
              )}
            </details>
          ))}
        </section>
        <aside className="opponent-panel">
          <p className="eyebrow">Test your strategy</p>
          <h2>Available opponents</h2>
          {opponents.isPending && <Loading label="Loading opponents…" />}
          {opponents.isError && (
            <Failure
              error={opponents.error}
              retry={() => void opponents.refetch()}
            />
          )}
          <ul className="opponent-list">
            {opponents.data?.map((opponent) => (
              <li key={opponent.id}>
                <strong>{opponent.name}</strong>
                <span>
                  {opponent.levels.length
                    ? `Levels ${opponent.levels[0].level}–${opponent.levels.at(-1)?.level}`
                    : "Seeded baseline"}
                </span>
                {opponent.levels.length > 0 && (
                  <small>
                    {opponent.calibrated
                      ? "Calibrated ladder"
                      : "Strength calibration pending"}
                  </small>
                )}
              </li>
            ))}
          </ul>
        </aside>
      </div>
    </>
  );
}
