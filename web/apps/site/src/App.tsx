import { Library, GameDetails } from "./library";
import { useRoute } from "./navigation";
import { MatchSetup } from "./setup";
import { MatchPage } from "./match";
import { HistoryPage } from "./history";
import { Spectator } from "./spectate";
import { ModelsPage } from "./models";
import { ReportsPage } from "./reports";
import { ComparisonPage } from "./report-comparison";
import { ReplayPage } from "./replay";

export function App() {
  const route = useRoute();
  const [path, search = ""] = route.split("?");
  const params = new URLSearchParams(search);
  let page = <Library />;
  const match = /^\/(games|matches)\/([^/]+)(\/(?:play|watch|replay))?$/.exec(
    path,
  );
  if (match) {
    try {
      const id = decodeURIComponent(match[2]);
      if (match[1] === "games") {
        if (match[3] && match[3] !== "/play")
          throw new Error("Invalid game link");
        page = match[3] ? (
          <MatchSetup key={id} id={id} />
        ) : (
          <GameDetails key={id} id={id} />
        );
      } else if (match[3] === "/replay") {
        const turn = Number(params.get("turn") ?? 0);
        const seat = params.get("seat") ?? "public";
        if (
          !Number.isSafeInteger(turn) ||
          turn < 0 ||
          (seat !== "public" && (!/^\d+$/.test(seat) || Number(seat) > 255))
        )
          throw new Error("Invalid replay link");
        page = (
          <ReplayPage
            key={id}
            id={id}
            initialTurn={turn}
            initialSeat={seat === "public" ? undefined : Number(seat)}
          />
        );
      } else if (match[3] === "/watch") {
        page = <Spectator key={id} id={id} />;
      } else {
        const seat = Number(params.get("seat") ?? 0);
        if (!Number.isInteger(seat) || seat < 0 || seat > 255 || match[3])
          throw new Error("Invalid match link");
        page = (
          <MatchPage
            key={id + search}
            id={id}
            initialSeat={seat}
            hotseat={params.get("hotseat") === "1"}
          />
        );
      }
    } catch {
      page = (
        <p role="alert">
          This link is invalid. <a href="#/">Return to the library.</a>
        </p>
      );
    }
  } else if (path === "/history" || path === "/live") {
    page = <HistoryPage key={path} live={path === "/live"} />;
  } else if (path === "/models") {
    page = <ModelsPage />;
  } else if (path === "/reports") {
    page = <ReportsPage />;
  } else if (path === "/reports/compare") {
    page = <ComparisonPage />;
  } else if (path !== "/") {
    page = (
      <section className="feedback">
        <h1>Page not found</h1>
        <p>
          <a href="#/">Return to the game library.</a>
        </p>
      </section>
    );
  }
  return (
    <div className="app-shell">
      <a
        className="skip-link"
        href="#main-content"
        onClick={(event) => {
          event.preventDefault();
          document.getElementById("main-content")?.focus();
        }}
      >
        Skip to content
      </a>
      <header className="app-header">
        <a className="wordmark" href="#/">
          <span className="brand-mark" aria-hidden="true">
            g.
          </span>
          GamesForAI
        </a>
        <nav aria-label="Main navigation">
          <a href="#/" aria-current={route === "/" ? "page" : undefined}>
            Library
          </a>
          <a href="#/live" aria-current={path === "/live" ? "page" : undefined}>
            Live
          </a>
          <a
            href="#/models"
            aria-current={path === "/models" ? "page" : undefined}
          >
            Models
          </a>
          <a
            href="#/reports"
            aria-current={path.startsWith("/reports") ? "page" : undefined}
          >
            Reports
          </a>
          <a
            href="#/history"
            aria-current={path === "/history" ? "page" : undefined}
          >
            History
          </a>
          <a href="/docs" target="_blank" rel="noreferrer">
            API reference <span aria-hidden="true">↗</span>
          </a>
        </nav>
        <span className="badge">
          <span className="status-dot" />
          Local workspace
        </span>
      </header>
      <main id="main-content" tabIndex={-1}>
        {page}
      </main>
      <footer>
        <span>GamesForAI</span>
        <span>Shared rules. Reproducible play.</span>
      </footer>
    </div>
  );
}
