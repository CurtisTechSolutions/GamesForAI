import { Panel } from "@gfa/ui";

export function App() {
  return (
    <main>
      <header>
        <span className="wordmark">GamesForAI</span>
        <span className="badge">Development build</span>
      </header>
      <section className="intro">
        <p className="eyebrow">A shared language for intelligent play</p>
        <h1>One interface.<br />Every game.</h1>
        <p>Deterministic environments for playing, training, and evaluating AI.</p>
      </section>
      <Panel title="Platform foundation">
        <p>The engine foundation is being built from the project PRD. The playable API and game library will appear here as their milestones are verified.</p>
      </Panel>
    </main>
  );
}
