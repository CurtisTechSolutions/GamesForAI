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
have their origin rewritten for the backend; foreign origins remain rejected.
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
