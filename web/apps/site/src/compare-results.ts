import type { TournamentReport } from "./tournament-report";

export interface OpponentResult {
  wins: number;
  draws: number;
  losses: number;
  excluded: number;
  schedule: string[];
}

export function resultsByOpponent(report: TournamentReport, model: string) {
  const results = new Map<string, OpponentResult>();
  for (const game of report.episodes) {
    const seat = game.agents.indexOf(model);
    if (seat < 0) continue;
    const opponent = game.agents[1 - seat];
    const result = results.get(opponent) ?? {
      wins: 0,
      draws: 0,
      losses: 0,
      excluded: 0,
      schedule: [],
    };
    result.schedule.push(JSON.stringify([game.seed, seat, game.position]));
    if (game.status === "completed" && game.returns) {
      const left = game.returns[seat],
        right = game.returns[1 - seat];
      result[left > right ? "wins" : left < right ? "losses" : "draws"]++;
    } else {
      result.excluded++;
    }
    results.set(opponent, result);
  }
  for (const result of results.values()) result.schedule.sort();
  return results;
}

export function resultScore(result?: OpponentResult) {
  if (!result) return null;
  const rated = result.wins + result.draws + result.losses;
  return rated ? (result.wins + result.draws / 2) / rated : null;
}

function canonical(value: unknown): string {
  if (Array.isArray(value)) return "[" + value.map(canonical).join(",") + "]";
  if (value && typeof value === "object")
    return (
      "{" +
      Object.entries(value)
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
        .map(([key, item]) => JSON.stringify(key) + ":" + canonical(item))
        .join(",") +
      "}"
    );
  return JSON.stringify(value) ?? "null";
}

export function settingDifferences(
  left: TournamentReport,
  right: TournamentReport,
) {
  const checks: [string, unknown, unknown][] = [
    ["game", left.game, right.game],
    ["engine version", left.engine, right.engine],
    [
      "game options",
      canonical(JSON.parse(left.config)),
      canonical(JSON.parse(right.config)),
    ],
    [
      "position set",
      left.positionSet?.sha256 ?? null,
      right.positionSet?.sha256 ?? null,
    ],
  ];
  const differences = checks
    .filter(([, a, b]) => a !== b)
    .map(([name]) => name);
  if (left.positionSet === undefined || right.positionSet === undefined)
    differences.push("position-set provenance missing");
  return differences;
}

export function matchingSchedules(left: OpponentResult, right: OpponentResult) {
  return (
    left.schedule.length === right.schedule.length &&
    left.schedule.every((entry, index) => entry === right.schedule[index])
  );
}
