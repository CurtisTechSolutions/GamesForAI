import { ApiError } from "@gfa/api-client";

export function Failure({
  error,
  retry,
}: {
  error: unknown;
  retry?: () => void;
}) {
  return (
    <section className="feedback error" role="alert">
      <h2>We couldn't load this.</h2>
      <p>
        {error instanceof ApiError
          ? error.message
          : "The game server isn't reachable. Start gfa serve and try again."}
      </p>
      {error instanceof ApiError && <p>{error.hint}</p>}
      {retry && (
        <button type="button" onClick={retry}>
          Try again
        </button>
      )}
    </section>
  );
}

export function Loading({ label = "Loading games…" }: { label?: string }) {
  return (
    <p className="loading" role="status">
      {label}
    </p>
  );
}
