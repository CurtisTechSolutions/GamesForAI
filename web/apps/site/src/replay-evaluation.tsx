import { useId } from "react";
import type { RecordedEvent } from "@gfa/api-client";

/** Display only recorded, disclosed estimates; never invent missing evaluations. */
export function EvaluationGraph({
  events,
  names,
  range,
  last,
}: {
  events: RecordedEvent[];
  names: string[];
  range: number[];
  last: number;
}) {
  const label = useId();
  const points = events.flatMap((event) => {
    if (
      event.type !== "action" ||
      !event.opponent_info ||
      typeof event.opponent_info !== "object" ||
      Array.isArray(event.opponent_info)
    )
      return [];
    const value = event.opponent_info.evaluation;
    return typeof value === "number" && Number.isFinite(value)
      ? [{ turn: event.turn, seat: event.seat, value }]
      : [];
  });
  if (!points.length)
    return (
      <p className="history-note">
        No engine evaluations were recorded for this view.
      </p>
    );
  const low = Math.min(range[0] ?? -1, ...points.map((point) => point.value));
  const high = Math.max(range[1] ?? 1, ...points.map((point) => point.value));
  const y = (value: number) =>
    140 - ((value - low) / Math.max(high - low, 1)) * 120;
  const x = (turn: number) => 45 + (turn / Math.max(last, 1)) * 510;
  return (
    <figure className="evaluation-graph">
      <figcaption id={label}>Recorded engine estimates</figcaption>
      <svg viewBox="0 0 600 170" role="img" aria-labelledby={label}>
        <line x1={45} y1={y(0)} x2={555} y2={y(0)} stroke="#526879" />
        <text x={5} y={25} fill="#adbaca" fontSize={13}>
          {high}
        </text>
        <text x={5} y={145} fill="#adbaca" fontSize={13}>
          {low}
        </text>
        <text x={45} y={165} fill="#adbaca" fontSize={13}>
          Turn 0
        </text>
        <text x={510} y={165} fill="#adbaca" fontSize={13}>
          Turn {last}
        </text>
        {names.map((name, seat) => (
          <polyline
            key={name}
            points={points
              .filter((point) => point.seat === seat)
              .map((point) => x(point.turn) + "," + y(point.value))
              .join(" ")}
            stroke={seat === 0 ? "#abefd0" : "#f4cd88"}
            strokeDasharray={seat === 0 ? undefined : "5 3"}
            strokeWidth={2}
            fill="none"
          />
        ))}
        {points.map((point) => (
          <circle
            key={point.turn}
            cx={x(point.turn)}
            cy={y(point.value)}
            r={4}
            fill={point.seat === 0 ? "#abefd0" : "#f4cd88"}
          >
            <title>
              {names[point.seat]} before turn {point.turn + 1}: {point.value}
            </title>
          </circle>
        ))}
      </svg>
      <p className="history-note">
        Expected return from the acting player's perspective. Solid line:{" "}
        {names[0]}. {names[1] ? "Dashed line: " + names[1] + "." : ""} Missing
        turns have no recorded estimate.
      </p>
      <details>
        <summary>Evaluation values</summary>
        <table>
          <thead>
            <tr>
              <th>Before turn</th>
              <th>Player</th>
              <th>Estimate</th>
            </tr>
          </thead>
          <tbody>
            {points.map((point) => (
              <tr key={point.turn}>
                <td>{point.turn + 1}</td>
                <td>{names[point.seat]}</td>
                <td>{point.value.toFixed(3)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </figure>
  );
}
