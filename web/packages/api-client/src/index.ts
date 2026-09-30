/** The only frontend package allowed to perform network requests. */
export class ApiClient {
  constructor(private readonly baseUrl = "") {}

  async request<T>(path: string, init?: RequestInit): Promise<T> {
    if (!path.startsWith("/v1/"))
      throw new Error("Expected a versioned API path");
    const response = await fetch(this.baseUrl + path, {
      ...init,
      headers: { "Content-Type": "application/json", ...init?.headers },
    });
    if (!response.ok) {
      const body: unknown = await response.json().catch(() => null);
      throw new ApiError(response.status, body);
    }
    return (await response.json()) as T;
  }
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly details: unknown,
  ) {
    super(`GamesForAI request failed (${status})`);
  }
}
