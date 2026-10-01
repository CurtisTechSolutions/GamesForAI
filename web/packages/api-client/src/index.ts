/** The only frontend package allowed to perform network requests. */
import type { Schemas } from "./generated";

export type { JsonValue, Schemas } from "./generated";
export type GameSpec = Schemas["GameSpec"];
export type Briefing = Schemas["Briefing"];
export type MatchState = Schemas["MatchState"];
export type CreatedMatch = Schemas["CreatedMatch"];
export type MatchMetadata = Schemas["MatchMetadata"];
export type MatchHistory = Schemas["MatchHistory"];
export type Replay = Schemas["Replay"];
export type LegalAction = Schemas["LegalAction"];
export type OpponentSpec = Schemas["OpponentSpec"];
export type RecordedEvent = Schemas["RecordedEvent"];

type Query = Record<string, string | number | boolean | undefined | null>;

function query(path: string, values: Query = {}) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null) params.set(key, String(value));
  }
  return path + (params.size ? "?" + params.toString() : "");
}

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export class ApiError extends Error {
  readonly code: string;
  readonly hint: string;
  constructor(
    public readonly status: number,
    public readonly details: unknown,
  ) {
    const error = object(details) && object(details.error) ? details.error : null;
    super(typeof error?.message === "string" ? error.message : `GamesForAI request failed (${status})`);
    this.name = "ApiError";
    this.code = typeof error?.code === "string" ? error.code : "REQUEST_FAILED";
    this.hint = typeof error?.hint === "string" ? error.hint : "Check the connection and try again.";
  }
}

async function boundedText(response: Response) {
  const limit = 16 * 1024 * 1024;
  if (Number(response.headers.get("Content-Length")) > limit) {
    await response.body?.cancel();
    throw new ApiError(502, { error: { code: "RESPONSE_TOO_LARGE" } });
  }
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.length;
    if (length > limit) {
      await reader.cancel();
      throw new ApiError(502, { error: { code: "RESPONSE_TOO_LARGE" } });
    }
    chunks.push(value);
  }
  const data = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    data.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(data);
}

export class ApiClient {
  constructor(private readonly baseUrl = "") {}

  private async send(path: string, init: RequestInit = {}) {
    if (!path.startsWith("/v1/") || path.includes("\\")) {
      throw new Error("Expected a versioned API path");
    }
    if (init.body && (typeof init.body !== "string" || new TextEncoder().encode(init.body).length > 65536)) {
      throw new Error("API JSON body exceeds 64 KiB");
    }
    const controller = new AbortController();
    const abort = () => controller.abort(init.signal?.reason);
    if (init.signal?.aborted) abort();
    else init.signal?.addEventListener("abort", abort, { once: true });
    const timeout = setTimeout(() => controller.abort(), 60000);
    try {
      const headers = new Headers(init.headers);
      headers.set("Content-Type", "application/json");
      const response = await fetch(this.baseUrl + path, {
        ...init, signal: controller.signal, headers,
        cache: "no-store", credentials: "same-origin", redirect: "error",
      });
      const text = await boundedText(response);
      if (!response.ok) {
        let body: unknown = null;
        try { body = JSON.parse(text); } catch { /* Non-JSON server failure. */ }
        throw new ApiError(response.status, body);
      }
      return text;
    } finally {
      clearTimeout(timeout);
      init.signal?.removeEventListener("abort", abort);
    }
  }

  async request<T>(path: string, init?: RequestInit): Promise<T> {
    const text = await this.send(path, init);
    try { return JSON.parse(text) as T; }
    catch { throw new ApiError(502, { error: { code: "INVALID_RESPONSE", message: "The server returned invalid JSON." } }); }
  }

  private post<T>(path: string, body: unknown, key?: string) {
    return this.request<T>(path, {
      method: "POST", body: JSON.stringify(body),
      headers: key ? { "Idempotency-Key": key } : undefined,
    });
  }

  games(signal?: AbortSignal) {
    return this.request<GameSpec[]>("/v1/games", { signal });
  }
  game(id: string, signal?: AbortSignal) {
    return this.request<GameSpec>("/v1/games/" + encodeURIComponent(id), { signal });
  }
  info(game: string, signal?: AbortSignal) {
    return this.request<Briefing>(query("/v1/games/" + encodeURIComponent(game) + "/info", { detail: "full" }), { signal });
  }
  prompt(game: string, signal?: AbortSignal) {
    return this.send(query("/v1/games/" + encodeURIComponent(game) + "/info", { format: "markdown", detail: "full" }), { signal });
  }
  opponents(game: string, signal?: AbortSignal) {
    return this.request<OpponentSpec[]>("/v1/games/" + encodeURIComponent(game) + "/opponents", { signal });
  }
  create(body: Schemas["CreateMatch"], seat = 0) {
    if (body.seed !== undefined && body.seed !== null && !Number.isSafeInteger(body.seed)) {
      throw new Error("The browser requires an exact safe-integer seed.");
    }
    return this.post<CreatedMatch>(query("/v1/matches", { seat }), body);
  }
  metadata(id: string, signal?: AbortSignal) {
    return this.request<MatchMetadata>("/v1/matches/" + encodeURIComponent(id), { signal });
  }
  state(id: string, seat?: number, signal?: AbortSignal) {
    return this.request<MatchState>(query("/v1/matches/" + encodeURIComponent(id) + "/state", { seat }), { signal });
  }
  move(id: string, body: Schemas["MoveRequest"], key = crypto.randomUUID()) {
    return this.post<Schemas["MoveResult"]>("/v1/matches/" + encodeURIComponent(id) + "/actions", body, key);
  }
  history(filters: Query = {}, signal?: AbortSignal) {
    return this.request<MatchHistory>(query("/v1/matches", filters), { signal });
  }
  events(id: string, seat?: number, after?: number, signal?: AbortSignal) {
    return this.request<Schemas["EventPage"]>(query("/v1/matches/" + encodeURIComponent(id) + "/events", { seat, after }), { signal });
  }
  replay(id: string, seat?: number, signal?: AbortSignal) {
    return this.request<Replay>(query("/v1/matches/" + encodeURIComponent(id) + "/replay", { seat }), { signal });
  }
  fork(id: string, body: Schemas["ForkMatch"], seat = 0) {
    return this.post<CreatedMatch>(query("/v1/matches/" + encodeURIComponent(id) + "/fork", { seat }), body);
  }
  control(id: string, kind: "resign" | "offer-draw", body: Schemas["ControlRequest"], key = crypto.randomUUID()) {
    return this.post<MatchState>("/v1/matches/" + encodeURIComponent(id) + "/" + kind, body, key);
  }
  analyze(body: Schemas["AnalysisRequest"], seat: number) {
    return this.post<Schemas["AnalysisResult"]>(query("/v1/analysis", { seat }), body);
  }
  validate(game: string, body: Schemas["ValidatePosition"], seat = 0) {
    return this.post<Schemas["ValidatedPosition"]>(query("/v1/games/" + encodeURIComponent(game) + "/positions/validate", { seat }), body);
  }

  /** Read-only stream. Disposal closes the socket and cancels reconnect timers. */
  watch(
    id: string,
    seat: number | undefined,
    receive: (state: MatchState) => void,
    status: (value: "connecting" | "live" | "reconnecting" | "closed") => void,
  ) {
    let stopped = false;
    let socket: WebSocket | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let delay = 500;
    const path = query("/v1/matches/" + encodeURIComponent(id) + "/stream", { seat });
    const url = new URL(this.baseUrl + path, window.location.href);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const open = () => {
      if (stopped) return;
      status(delay === 500 ? "connecting" : "reconnecting");
      socket = new WebSocket(url);
      socket.onopen = () => { delay = 500; status("live"); };
      socket.onmessage = (event) => {
        try {
          if (typeof event.data !== "string" || event.data.length > 16 * 1024 * 1024) throw new Error("Invalid stream frame");
          const message: unknown = JSON.parse(event.data);
          if (!object(message) || message.type !== "state" || !object(message.state)) throw new Error("Stream closed");
          if (message.state.match_id !== id || !Number.isSafeInteger(message.state.turn)) throw new Error("Wrong stream state");
          receive(message.state as MatchState);
        } catch { socket?.close(); }
      };
      socket.onclose = () => {
        if (stopped) return;
        status("reconnecting");
        timer = setTimeout(open, delay);
        delay = Math.min(delay * 2, 10000);
      };
    };
    open();
    return () => {
      stopped = true;
      clearTimeout(timer);
      socket?.close();
      status("closed");
    };
  }
}
