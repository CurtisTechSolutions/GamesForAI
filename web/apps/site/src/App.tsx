import { Library, GameDetails } from "./library";
import { useRoute } from "./navigation";

export function App() {
  const route = useRoute();
  let page = <Library />;
  const match = /^\/games\/([^/]+)$/.exec(route);
  if (match) {
    try { page = <GameDetails key={match[1]} id={decodeURIComponent(match[1])} />; }
    catch { page = <p role="alert">This game link is invalid. <a href="#/">Return to the library.</a></p>; }
  } else if (route !== "/") {
    page = <section className="feedback"><h1>Page not found</h1><p><a href="#/">Return to the game library.</a></p></section>;
  }
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content" onClick={(event) => { event.preventDefault(); document.getElementById("main-content")?.focus(); }}>Skip to content</a>
      <header className="app-header">
        <a className="wordmark" href="#/"><span className="brand-mark" aria-hidden="true">g.</span>GamesForAI</a>
        <nav aria-label="Main navigation"><a href="#/" aria-current={route === "/" ? "page" : undefined}>Library</a><a href="/docs" target="_blank" rel="noreferrer">API reference <span aria-hidden="true">↗</span></a></nav>
        <span className="badge"><span className="status-dot" />Local workspace</span>
      </header>
      <main id="main-content" tabIndex={-1}>{page}</main>
      <footer><span>GamesForAI</span><span>Shared rules. Reproducible play.</span></footer>
    </div>
  );
}
