// Browser boundary for the versioned Python tournament report. Keep uploaded
// data bounded and render only validated fields; no file paths become links.
export interface Standing {
  agent: string;
  wins: number;
  draws: number;
  losses: number;
  ratedGames: number;
  failed: number;
  truncated: number;
  invalidStarts: number;
  rating: number;
  prior: number;
  interval: [number, number];
  score: number | null;
}
export interface Episode {
  id: string;
  agents: [string, string];
  seed: string;
  turns: number;
  status: "completed" | "truncated" | "failed" | "invalid_start";
  returns: [number, number] | null;
  failure: string | null;
  position: string | null;
}
export interface TournamentReport {
  id: string;
  game: string;
  engine: string;
  createdAt: string;
  seed: string;
  gamesPerPair: number;
  config: string;
  standings: Standing[];
  episodes: Episode[];
}
export const reportByteLimit = 8 * 1024 * 1024;
function invalid(): never {
  throw new Error(
    "This file is not a valid GamesForAI tournament report (format_version 1).",
  );
}
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    return invalid();
  return value as Record<string, unknown>;
}
function text(value: unknown, max = 256): string {
  if (typeof value !== "string" || !value || value.length > max)
    return invalid();
  return value;
}
function finite(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return invalid();
  return value;
}
function count(value: unknown, max = 10000): number {
  const number = finite(value);
  if (!Number.isSafeInteger(number) || number < 0 || number > max)
    return invalid();
  return number;
}
function list(value: unknown, max: number): unknown[] {
  if (!Array.isArray(value) || value.length > max) return invalid();
  return value;
}
function pair<T>(value: unknown, parse: (value: unknown) => T): [T, T] {
  const values = list(value, 2);
  if (values.length !== 2) return invalid();
  return [parse(values[0]), parse(values[1])];
}
function seed(value: unknown) {
  const result = text(value, 20);
  if (!/^\d+$/.test(result) || BigInt(result) > 18446744073709551615n)
    return invalid();
  return result;
}
function jsonOptions(value: unknown, depth = 0): void {
  if (depth > 24) return invalid();
  if (typeof value === "number") finite(value);
  if (Array.isArray(value))
    value.forEach((item) => jsonOptions(item, depth + 1));
  else if (value && typeof value === "object")
    Object.values(value).forEach((item) => jsonOptions(item, depth + 1));
}

export function parseTournamentReport(raw: string): TournamentReport {
  if (new TextEncoder().encode(raw).length > reportByteLimit)
    throw new Error("Choose a report smaller than 8 MiB.");
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error("Choose a valid JSON report from gfa tournament.");
  }
  const root = record(value);
  if (
    root.format_version !== 1 ||
    record(root.rating_system).name !== "glicko2"
  )
    return invalid();
  const specs = list(root.agent_specs, 32).map((item) => text(record(item).id));
  const agents = new Set(specs);
  if (agents.size < 2 || agents.size !== specs.length) return invalid();
  const standings = list(root.standings, 32).map((item) => {
    const row = record(item),
      rating = record(row.rating),
      initial = record(row.initial_rating);
    const agent = text(row.agent);
    if (!agents.has(agent)) return invalid();
    const wins = count(row.wins),
      draws = count(row.draws),
      losses = count(row.losses),
      ratedGames = count(row.rated_games);
    if (ratedGames !== wins + draws + losses) return invalid();
    const estimate = finite(rating.rating),
      interval = pair(row.rating_interval95, finite);
    if (interval[0] > estimate || interval[1] < estimate) return invalid();
    const score = row.score === null ? null : finite(row.score);
    if (
      (ratedGames === 0 && score !== null) ||
      (ratedGames > 0 &&
        (score === null ||
          Math.abs(score - (wins + draws / 2) / ratedGames) > 1e-6))
    )
      return invalid();
    return {
      agent,
      wins,
      draws,
      losses,
      ratedGames,
      score,
      rating: estimate,
      interval,
      prior: finite(initial.rating),
      failed: count(row.failed),
      truncated: count(row.truncated),
      invalidStarts: count(row.invalid_starts),
    };
  });
  if (
    standings.length !== agents.size ||
    new Set(standings.map((row) => row.agent)).size !== agents.size
  )
    return invalid();
  const episodes: Episode[] = list(root.matches, 10000).map((item) => {
    const row = record(item);
    const participants = pair(row.agents, text);
    if (
      participants[0] === participants[1] ||
      participants.some((agent) => !agents.has(agent))
    )
      return invalid();
    const status = text(row.status);
    if (
      status !== "completed" &&
      status !== "truncated" &&
      status !== "failed" &&
      status !== "invalid_start"
    )
      return invalid();
    const returns = row.returns === null ? null : pair(row.returns, finite);
    if ((status === "completed" || status === "truncated") && returns === null)
      return invalid();
    const failure =
      row.failure === null ? null : text(record(row.failure).code, 128);
    return {
      id: text(row.id, 300),
      agents: participants,
      seed: seed(row.seed),
      turns: count(row.turns, Number.MAX_SAFE_INTEGER),
      status,
      returns,
      failure,
      position: row.position_id === null ? null : text(row.position_id),
    };
  });
  if (new Set(episodes.map((row) => row.id)).size !== episodes.length)
    return invalid();
  const fields = [
    "wins",
    "draws",
    "losses",
    "ratedGames",
    "failed",
    "truncated",
    "invalidStarts",
  ] as const;
  const observed = new Map(
    standings.map((row) => [
      row.agent,
      {
        wins: 0,
        draws: 0,
        losses: 0,
        ratedGames: 0,
        failed: 0,
        truncated: 0,
        invalidStarts: 0,
      },
    ]),
  );
  for (const episode of episodes) {
    for (const [seat, agent] of episode.agents.entries()) {
      const totals = observed.get(agent);
      if (!totals) return invalid();
      if (episode.status === "completed" && episode.returns) {
        totals.ratedGames++;
        const left = episode.returns[seat],
          right = episode.returns[1 - seat];
        totals[left > right ? "wins" : left < right ? "losses" : "draws"]++;
      } else {
        totals[
          episode.status === "failed"
            ? "failed"
            : episode.status === "truncated"
              ? "truncated"
              : "invalidStarts"
        ]++;
      }
    }
  }
  for (const row of standings) {
    const totals = observed.get(row.agent);
    if (!totals || fields.some((field) => totals[field] !== row[field]))
      return invalid();
  }
  const createdAt = text(root.created_at, 80);
  if (!Number.isFinite(new Date(createdAt).getTime())) return invalid();
  const gamesPerPair = count(root.games_per_pair, 1000);
  if (gamesPerPair < 2 || gamesPerPair % 2) return invalid();
  const config = record(root.config);
  jsonOptions(config);
  const configText = JSON.stringify(config, null, 2);
  if (configText.length > 32768) return invalid();
  return {
    id: text(root.run_id),
    game: text(root.game),
    engine: text(root.engine_version),
    createdAt,
    seed: seed(root.seed),
    gamesPerPair,
    config: configText,
    standings,
    episodes,
  };
}

export const episodeLabels: Record<Episode["status"], string> = {
  completed: "Completed",
  truncated: "Move limit reached",
  failed: "Policy failed",
  invalid_start: "Invalid start",
};

export function ratingScale(standings: Standing[]) {
  const low = Math.min(...standings.map((row) => row.interval[0]));
  const high = Math.max(...standings.map((row) => row.interval[1]));
  // Normalize first, so even a finite but extreme uploaded rating cannot
  // overflow the subtraction and put NaN coordinates into the SVG.
  const unit = Math.max(Math.abs(low), Math.abs(high), 1);
  const span = high / unit - low / unit;
  return {
    low,
    high,
    x: (value: number) =>
      span === 0 ? 50 : 5 + (90 * (value / unit - low / unit)) / span,
  };
}
