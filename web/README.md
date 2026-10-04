# Browser application

The library reads the installed game catalog, play guides and opponent ladders
from the running server. Each guide can be copied as a model prompt. The app
includes search, keyboard navigation, error recovery and a layout down to 360px.

Use Node.js 22 and pnpm 10.6.5. Start the backend in one terminal:

```sh
cargo run -p gfa-cli --locked -- serve --sqlite ./gfa.sqlite --port 8080
```

Then start the browser application from the repository root:

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Open the local URL printed by Vite. The development proxy forwards API and
WebSocket requests to `http://127.0.0.1:8080`. Override `GFA_API_TARGET` with
another local HTTP backend if needed. Only requests from the app's own origin
have their origin rewritten for the backend; foreign origins are replaced with an invalid origin and rejected by the backend.
This development server is for a trusted local workspace.

## Architecture and checks

- `apps/site`: React routing, panels and TanStack Query server state.
- `packages/api-client`: typed HTTP/WebSocket boundary, with generated Rust
  OpenAPI types and bounded requests/responses.
- `packages/ui`: shared components.
- `packages/game-kit`: Phaser lifecycle and accessible text controls. Scenes
  render observations and submit legal actions; game rules stay in Rust.

```sh
pnpm typecheck
pnpm lint
pnpm format:check
pnpm build
cargo build -p gfa-cli --locked
pnpm test:browser
```

Browser tests start isolated backend and app servers, use Chromium, and exercise
the actual catalog, guides, clipboard, mobile/keyboard behavior, errors and local
HTTP/WebSocket origin guards. CI retains failure traces and screenshots.

## Match play

Use a game's **Play** link to start a hot-seat match, choose an installed
opponent, or leave a seat open for your own REST/MCP agent. Seeds are exact
nonnegative JavaScript safe integers. Custom notation and JSON game options
are checked by the server; you can validate a custom position before creation.

The match room offers an accessible text board, legal-move list and notation
entry, a live connection with polling fallback, move/reasoning history,
resignation and draw controls. Engine hints default off and must be allowed
when creating the match. Retries of an uncertain move reuse its request key,
turn and seat; mutations never retry automatically. Local mode treats this
browser as the match owner; seat selection is a view, not authentication.

## Visual boards

Tic-Tac-Toe, Connect Four, chess and Sudoku have Phaser scene packages. The game
kit loads Phaser and each scene on demand; library and guide pages do not load
the renderer. Scenes draw only server observations and submit actions from the
server's legal list. They contain no game rules or network requests.

Click/tap a cell, or focus the board and use arrow keys and Enter. Connect Four
also accepts column keys 1–7. Chess highlights legal destinations and asks for a
promotion piece. Sudoku supports notes (N), digit keys and an accessible touch
keypad; givens cannot be changed. The text board and notation input remain
available, including when a renderer cannot load.

Add a scene in `packages/game-<id>` using `BoardScene` from `@gfa/game-kit/scene`,
then add its lazy loader to `packages/game-kit/src/registry.ts`. The site uses
only the game-kit interface. Unregistered games keep the generic text renderer.

## History and live spectating

**History** lists local matches with game/status filters and cursor pagination.
Search and result filters operate on the loaded records; use **Load more matches**
to continue the search. Match cards link back to controls or to a spectator view.

**Live** lists active matches and shows up to four selected boards in an arena.
A spectator link (`#/matches/<id>/watch`) requests only the public observation
and public events. It never submits moves or requests a player's private view.
WebSocket updates refresh the boards and move lists; polling continues when the
stream is unavailable. Provider reasoning remains subject to server disclosure
rules. This is the local workspace; hosted visibility/account controls are not
implemented by these pages.

## Replays and variations

Open **Replay** from history or **Replay and branch** from a match. The replay
uses recorded states, with timeline/step controls, autoplay speed, a public or
player perspective, and links that retain the selected turn. The move list
shows disclosed reasoning and recorded agent diagnostics. Engine estimates
are plotted only where they were recorded, from the acting player's perspective;
the viewer does not invent evaluations or blunder labels for missing analysis.

**Branch from here** creates an independent match from the selected turn, with
hot-seat, an installed opponent, or an open agent seat. Game options and assists
are inherited. Parent matches remain unchanged. The variation panel links the
ancestor chain and discovered direct children; **Find more variations** continues
the history scan when more records exist. Provider transcripts and omniscient
hidden-information views require their corresponding backend capabilities.
