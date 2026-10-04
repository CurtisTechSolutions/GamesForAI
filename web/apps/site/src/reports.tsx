import { useRef, useState } from "react";
import {
  parseTournamentReport,
  reportByteLimit,
  ratingScale,
  episodeLabels,
  type TournamentReport,
  type Episode,
} from "./tournament-report";
import { downloadText } from "./export-controls";
import "./reports.css";

const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
const score = (value: number | null) =>
  value === null ? "No rated games" : (value * 100).toFixed(1) + "%";
const gameNames = new Map([
  ["tictactoe", "Tic-Tac-Toe"],
  ["connect4", "Connect Four"],
  ["chess", "Chess"],
  ["sudoku", "Sudoku"],
]);
type Loaded = {
  report: TournamentReport;
  raw: string;
  name: string;
  example: boolean;
  revision: number;
};

export function ReportsPage() {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const request = useRef(0);
  const load = async (file?: File) => {
    const current = ++request.current;
    setError("");
    setBusy(true);
    try {
      if (file && file.size > reportByteLimit)
        throw new Error("Choose a report smaller than 8 MiB.");
      const raw = file
        ? await file.text()
        : JSON.stringify((await import("./report-example.json")).default);
      const report = parseTournamentReport(raw);
      if (current === request.current)
        setLoaded({
          report,
          raw,
          name: file?.name.slice(0, 256) ?? "Example tournament",
          example: !file,
          revision: current,
        });
    } catch (failure) {
      if (current === request.current)
        setError(
          failure instanceof Error
            ? failure.message
            : "This report could not be opened.",
        );
    } finally {
      if (current === request.current) setBusy(false);
    }
  };
  return (
    <>
      <section className="game-heading reports-page-heading">
        <div>
          <p className="eyebrow">Learn from every run</p>
          <h1>Evaluation reports</h1>
          <p>
            See how your models performed, where games failed, and how much
            evidence supports a rating.
          </p>
        </div>
        <a className="button secondary" href="#/models">
          Prepare a run
        </a>
      </section>
      <section className="report-import" aria-label="Open an evaluation report">
        <div>
          <h2>Open your tournament report</h2>
          <p>
            Choose the JSON file produced by your local runner. Its contents
            stay in this tab.
          </p>
        </div>
        <div className="report-import-actions">
          <label className="button report-file">
            Open JSON report
            <input
              type="file"
              accept=".json,application/json"
              aria-label="Open JSON report"
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = "";
                if (file) void load(file);
              }}
            />
          </label>
          <button className="secondary" onClick={() => void load()}>
            Explore an example
          </button>
        </div>
      </section>
      {busy && <p role="status">Reading report…</p>}
      {error && (
        <section className="feedback error" role="alert">
          <h2>We couldn't open that report.</h2>
          <p>{error}</p>
        </section>
      )}
      {loaded ? (
        <ReportView key={loaded.revision} loaded={loaded} />
      ) : (
        !busy && (
          <section className="report-empty">
            <div className="report-empty-bars" aria-hidden="true">
              <i />
              <i />
              <i />
              <i />
              <i />
            </div>
            <h2>A clearer view of your next experiment</h2>
            <p>
              Compare wins and draws, inspect rating uncertainty, and filter
              individual games by model or outcome.
            </p>
            <p className="report-note">
              Run <code>gfa tournament</code> from the Models page, then open
              tournament-report.json here. Keep the file to reopen the report
              later.
            </p>
          </section>
        )
      )}
    </>
  );
}

function ReportView({ loaded }: { loaded: Loaded }) {
  const { report } = loaded;
  const [agent, setAgent] = useState("");
  const [status, setStatus] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const filtered = report.episodes.filter(
    (row) =>
      (!agent || row.agents.includes(agent)) &&
      (!status || row.status === status) &&
      (!search ||
        (row.id + " " + row.seed + " " + row.agents.join(" "))
          .toLowerCase()
          .includes(search.toLowerCase())),
  );
  const lastPage = Math.max(0, Math.ceil(filtered.length / 25) - 1);
  const currentPage = Math.min(page, lastPage);
  const rows = filtered.slice(currentPage * 25, currentPage * 25 + 25);
  const completed = report.episodes.filter(
    (row) => row.status === "completed",
  ).length;
  const failed = report.episodes.filter(
    (row) => row.status === "failed",
  ).length;
  const scale = ratingScale(report.standings);
  return (
    <div className="report-content">
      <section className="report-heading">
        <div>
          <span className={"report-badge" + (loaded.example ? " example" : "")}>
            {loaded.example
              ? "Example data · native test policies"
              : "Local tournament report"}
          </span>
          <h2>{gameNames.get(report.game) ?? report.game}</h2>
          <p>
            {loaded.name} · {new Date(report.createdAt).toLocaleString()}
          </p>
        </div>
        <button
          className="secondary"
          onClick={() => downloadText("tournament-report.json", loaded.raw)}
        >
          Download report
        </button>
      </section>
      <div className="report-metrics" aria-label="Run totals">
        {[
          ["Games", report.episodes.length],
          ["Completed", completed],
          ["Policy failures", failed],
          ["Models", report.standings.length],
        ].map(([label, value]) => (
          <div key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
          </div>
        ))}
      </div>
      <section className="report-panel" aria-labelledby="report-standings">
        <div className="report-section-heading">
          <div>
            <p className="eyebrow">Results that count</p>
            <h2 id="report-standings">Model standings</h2>
          </div>
          <span className="report-note">
            Score = (wins + half of draws) / rated games
          </span>
        </div>
        <div
          className="report-table-scroll"
          role="region"
          aria-label="Standings table"
          tabIndex={0}
        >
          <table>
            <thead>
              <tr>
                <th>Model snapshot</th>
                <th>Wins</th>
                <th>Draws</th>
                <th>Losses</th>
                <th>Score</th>
                <th>Glicko-2</th>
                <th>Rated games</th>
              </tr>
            </thead>
            <tbody>
              {report.standings.map((row) => (
                <tr key={row.agent}>
                  <th scope="row">{row.agent}</th>
                  <td>{row.wins}</td>
                  <td>{row.draws}</td>
                  <td>{row.losses}</td>
                  <td>{score(row.score)}</td>
                  <td>{number.format(row.rating)}</td>
                  <td>{row.ratedGames}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="report-note">
          {report.episodes.length - completed} games excluded from ratings
          because they failed, reached a move limit, or started in a finished
          position.
        </p>
      </section>
      <section className="report-panel" aria-labelledby="rating-uncertainty">
        <p className="eyebrow">Look beyond one number</p>
        <h2 id="rating-uncertainty">Rating and uncertainty</h2>
        <p className="report-note">
          Dots show Glicko-2 estimates; lines show the runner's approximate 95%
          rating intervals. Wider intervals mean less evidence. Estimates depend
          on the opponents and initial ratings in this run.
        </p>
        <div className="rating-axis" aria-hidden="true">
          <span>{number.format(scale.low)}</span>
          <span>{number.format(scale.high)}</span>
        </div>
        {report.standings.map((row) => (
          <div className="report-rating-row" key={row.agent}>
            <div>
              <strong>{row.agent}</strong>
              <small>
                {number.format(row.interval[0])}–
                {number.format(row.interval[1])} · prior{" "}
                {number.format(row.prior)}
              </small>
            </div>
            <svg
              viewBox="0 0 100 12"
              role="img"
              aria-label={
                row.agent +
                ": rating " +
                number.format(row.rating) +
                ", interval " +
                number.format(row.interval[0]) +
                " to " +
                number.format(row.interval[1])
              }
            >
              <line
                x1={scale.x(row.interval[0])}
                x2={scale.x(row.interval[1])}
                y1="6"
                y2="6"
                stroke="currentColor"
                strokeWidth="1.5"
              />
              <circle
                cx={scale.x(row.rating)}
                cy="6"
                r="2.2"
                fill="currentColor"
              />
            </svg>
          </div>
        ))}
      </section>
      <section className="report-panel" aria-labelledby="report-episodes">
        <div className="report-section-heading">
          <div>
            <p className="eyebrow">Every game, accounted for</p>
            <h2 id="report-episodes">Game results</h2>
          </div>
          <span role="status">{filtered.length} matching games</span>
        </div>
        <div className="report-filters">
          <label>
            Filter model
            <select
              aria-label="Filter model"
              value={agent}
              onChange={(event) => {
                setAgent(event.target.value);
                setPage(0);
              }}
            >
              <option value="">All models</option>
              {report.standings.map((row) => (
                <option key={row.agent} value={row.agent}>
                  {row.agent}
                </option>
              ))}
            </select>
          </label>
          <label>
            Filter outcome
            <select
              aria-label="Filter outcome"
              value={status}
              onChange={(event) => {
                setStatus(event.target.value);
                setPage(0);
              }}
            >
              <option value="">All outcomes</option>
              {Object.entries(episodeLabels).map(([value, label]) => (
                <option value={value} key={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Search games
            <input
              value={search}
              maxLength={256}
              placeholder="Game ID, seed or model"
              onChange={(event) => {
                setSearch(event.target.value);
                setPage(0);
              }}
            />
          </label>
        </div>
        {rows.length ? (
          <div className="episode-list">
            {rows.map((row) => (
              <EpisodeRow key={row.id} row={row} />
            ))}
          </div>
        ) : (
          <p className="report-note" role="status">
            No games match these filters.
          </p>
        )}
        {filtered.length > 25 && (
          <nav className="report-pagination" aria-label="Game result pages">
            <button
              className="secondary"
              disabled={currentPage === 0}
              onClick={() => setPage(currentPage - 1)}
            >
              Previous games
            </button>
            <span>
              Page {currentPage + 1} of {lastPage + 1}
            </span>
            <button
              className="secondary"
              disabled={currentPage === lastPage}
              onClick={() => setPage(currentPage + 1)}
            >
              Next games
            </button>
          </nav>
        )}
      </section>
      <details className="report-panel report-provenance">
        <summary>Run configuration and provenance</summary>
        <dl>
          <dt>Run ID</dt>
          <dd>{report.id}</dd>
          <dt>Game / engine</dt>
          <dd>
            {report.game} / {report.engine}
          </dd>
          <dt>Schedule</dt>
          <dd>
            {report.gamesPerPair} games per selected pair, alternating seats
          </dd>
          <dt>Starting seed</dt>
          <dd>{report.seed}</dd>
        </dl>
        <h3>Game options</h3>
        <pre>{report.config}</pre>
        <p className="report-note">
          This is a local tournament report. Native evaluation games are
          identified by their run and episode; server match history and live
          replays are a separate record.
        </p>
      </details>
    </div>
  );
}

function EpisodeRow({ row }: { row: Episode }) {
  return (
    <details className="episode-result">
      <summary>
        <span className={"episode-status " + row.status}>
          {episodeLabels[row.status]}
        </span>
        <span className="episode-players">
          {row.agents[0]} <small>vs</small> {row.agents[1]}
        </span>
        <span className="episode-turns">{row.turns} turns</span>
      </summary>
      <dl>
        <dt>Game ID</dt>
        <dd>{row.id}</dd>
        <dt>Seed</dt>
        <dd>{row.seed}</dd>
        <dt>Seat 0 / seat 1</dt>
        <dd>{row.agents.join(" / ")}</dd>
        <dt>Returns</dt>
        <dd>{row.returns?.join(" / ") ?? "No completed result"}</dd>
        {row.failure && (
          <>
            <dt>Failure code</dt>
            <dd>{row.failure}</dd>
          </>
        )}
        {row.position && (
          <>
            <dt>Starting position ID</dt>
            <dd>{row.position}</dd>
          </>
        )}
      </dl>
    </details>
  );
}
