import { useRef, useState } from "react";
import {
  parseTournamentReport,
  reportByteLimit,
  type TournamentReport,
} from "./tournament-report";
import {
  matchingSchedules,
  resultScore,
  resultsByOpponent,
  settingDifferences,
  type OpponentResult,
} from "./compare-results";
import "./reports.css";
import "./report-comparison.css";

type Selection = { report: TournamentReport; name: string; model: string };
const percent = (score: number | null) =>
  score === null ? "No rated games" : (100 * score).toFixed(1) + "%";
const integer = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });

export function ComparisonPage() {
  const [left, setLeft] = useState<Selection | null>(null);
  const [right, setRight] = useState<Selection | null>(null);
  return (
    <>
      <a className="back-link" href="#/reports">
        ← Evaluation reports
      </a>
      <section className="game-heading reports-page-heading">
        <div>
          <p className="eyebrow">Follow your model's progress</p>
          <h1>Compare evaluations</h1>
          <p>
            Choose a model snapshot from each run and compare its results
            against shared opponents. Both files stay in this tab.
          </p>
        </div>
      </section>
      <div className="comparison-runs">
        <RunPicker label="A" value={left} onChange={setLeft} />
        <RunPicker label="B" value={right} onChange={setRight} />
      </div>
      {left && right ? (
        <ComparisonResults left={left} right={right} />
      ) : (
        <section className="comparison-empty" role="status">
          <h2>
            {left || right
              ? "Open the other run to compare results"
              : "Two runs. One view of your progress."}
          </h2>
          <p>
            Use tournament reports from before and after training. Select the
            snapshot you want to inspect in each run, even when their names
            differ.
          </p>
        </section>
      )}
    </>
  );
}

function RunPicker({
  label,
  value,
  onChange,
}: {
  label: string;
  value: Selection | null;
  onChange: (selection: Selection | null) => void;
}) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const request = useRef(0);
  const upload = async (file: File) => {
    const current = ++request.current;
    setBusy(true);
    setError("");
    try {
      if (file.size > reportByteLimit)
        throw new Error("Choose a report smaller than 8 MiB.");
      const report = parseTournamentReport(await file.text());
      if (current === request.current)
        onChange({
          report,
          name: file.name.slice(0, 256),
          model:
            report.standings.find((row) => row.agent === value?.model)?.agent ??
            report.standings[0].agent,
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
  const standing = value?.report.standings.find(
    (row) => row.agent === value.model,
  );
  return (
    <section
      className="report-panel comparison-run"
      aria-label={"Run " + label}
    >
      <div className="comparison-run-heading">
        <h2>Run {label}</h2>
        <label className="button secondary report-file">
          {value ? "Replace report" : "Open report"}
          <input
            type="file"
            accept=".json,application/json"
            aria-label={"Open run " + label + " report"}
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file) void upload(file);
            }}
          />
        </label>
      </div>
      {busy && <p role="status">Reading report…</p>}
      {error && (
        <p className="comparison-error" role="alert">
          {error}
        </p>
      )}
      {value && standing ? (
        <>
          <p className="comparison-filename">{value.name}</p>
          <p className="report-note">
            {value.report.game} · engine {value.report.engine} ·{" "}
            {new Date(value.report.createdAt).toLocaleString()}
          </p>
          <label className="comparison-model">
            Model in run {label}
            <select
              aria-label={"Model in run " + label}
              value={value.model}
              onChange={(event) =>
                onChange({ ...value, model: event.target.value })
              }
            >
              {value.report.standings.map((row) => (
                <option key={row.agent} value={row.agent}>
                  {row.agent}
                </option>
              ))}
            </select>
          </label>
          <div className="comparison-score">
            <strong>{percent(standing.score)}</strong>
            <span>score across {standing.ratedGames} rated games</span>
            <span>
              {standing.wins} wins · {standing.draws} draws · {standing.losses}{" "}
              losses
            </span>
          </div>
          <dl className="comparison-context">
            <dt>Reported rating</dt>
            <dd>
              {integer.format(standing.rating)}{" "}
              <small>
                ({integer.format(standing.interval[0])}–
                {integer.format(standing.interval[1])})
              </small>
            </dd>
            <dt>Initial rating</dt>
            <dd>{integer.format(standing.prior)}</dd>
            <dt>Excluded games</dt>
            <dd>
              {standing.failed + standing.truncated + standing.invalidStarts}
            </dd>
            <dt>Starting seed</dt>
            <dd>{value.report.seed}</dd>
            <dt>Position set</dt>
            <dd>
              {value.report.positionSet === undefined
                ? "Not recorded"
                : (value.report.positionSet?.id ?? "Standard start")}
            </dd>
          </dl>
          <details className="comparison-options">
            <summary>Run ID and game options</summary>
            <p>{value.report.id}</p>
            {value.report.positionSet && (
              <p>Position set checksum: {value.report.positionSet.sha256}</p>
            )}
            <pre>{value.report.config}</pre>
          </details>
        </>
      ) : (
        !busy && (
          <p className="report-note">
            Choose the JSON report saved by gfa tournament.
          </p>
        )
      )}
    </section>
  );
}

function ComparisonResults({
  left,
  right,
}: {
  left: Selection;
  right: Selection;
}) {
  const first = resultsByOpponent(left.report, left.model);
  const second = resultsByOpponent(right.report, right.model);
  const opponents = [...new Set([...first.keys(), ...second.keys()])].sort();
  const differences = settingDifferences(left.report, right.report);
  const shared = opponents.filter(
    (name) => first.has(name) && second.has(name),
  ).length;
  return (
    <section
      className="report-panel comparison-results"
      aria-labelledby="comparison-results-title"
    >
      <p className="eyebrow">Compare the same opponents</p>
      <h2 id="comparison-results-title">Results by opponent</h2>
      <p className="report-note" role="status">
        {shared} shared opponent{shared === 1 ? "" : "s"}
      </p>
      <p
        className={
          "comparison-conditions" + (differences.length ? " mismatch" : "")
        }
      >
        {differences.length
          ? "Settings differ or are incomplete: " +
            differences.join(", ") +
            ". Score changes are hidden."
          : "Score changes appear only for shared opponents with matching seeds, seats, and starting positions."}
      </p>
      <div
        className="report-table-scroll comparison-table"
        role="region"
        aria-label="Comparison by opponent"
        tabIndex={0}
      >
        <table>
          <thead>
            <tr>
              <th scope="col">Opponent snapshot</th>
              <th scope="col">Run A</th>
              <th scope="col">Run B</th>
              <th scope="col">Score change</th>
            </tr>
          </thead>
          <tbody>
            {opponents.map((opponent) => {
              const a = first.get(opponent),
                b = second.get(opponent);
              const aScore = resultScore(a),
                bScore = resultScore(b);
              const canCompare =
                !differences.length && a && b && matchingSchedules(a, b);
              return (
                <tr key={opponent}>
                  <th scope="row">{opponent}</th>
                  <td>
                    <ResultSummary value={a} />
                  </td>
                  <td>
                    <ResultSummary value={b} />
                  </td>
                  <td>
                    {canCompare && aScore !== null && bScore !== null ? (
                      <strong className="comparison-delta">
                        {bScore > aScore ? "+" : ""}
                        {((bScore - aScore) * 100).toFixed(1)} pp
                      </strong>
                    ) : (
                      <span className="comparison-unavailable">
                        {!a || !b
                          ? "No shared matchup"
                          : differences.length
                            ? "Settings differ"
                            : !canCompare
                              ? "Schedules differ"
                              : "No rated games"}
                      </span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {!shared && (
        <p className="report-note">
          These snapshots did not play any of the same opponents. Run both
          against the same opponent snapshots to compare their results.
        </p>
      )}
      <p className="report-note">
        Score counts a win as 1 and a draw as ½. Changes are run B minus run A
        in percentage points (pp), describing these games rather than
        statistical significance. Failed, truncated, and invalid-start games are
        excluded.
      </p>
      <p className="report-note">
        Overall scores and ratings also depend on the full opponent mix and
        rating priors. Use a new snapshot name when model weights change.
      </p>
    </section>
  );
}

function ResultSummary({ value }: { value?: OpponentResult }) {
  if (!value) return <span className="comparison-unavailable">Not played</span>;
  return (
    <div className="opponent-result">
      <strong>{percent(resultScore(value))}</strong>
      <span>
        {value.wins} W · {value.draws} D · {value.losses} L
      </span>
      <small>{value.excluded} excluded</small>
    </div>
  );
}
