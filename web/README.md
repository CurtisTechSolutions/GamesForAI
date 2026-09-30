# Frontend workspace

This is the M0 frontend foundation. It is a development shell, not a playable application.

- `apps/site`: React/Vite shell, with TanStack Query ready for server data.
- `packages/api-client`: the only package permitted to call the API.
- `packages/ui`: shared accessible components.
- `packages/game-kit`: Phaser scene lifecycle/registry and accessible text controls. Scenes render server observations and submit the server's legal actions; they do not implement game rules.

From the repository root, use Node.js 22 and pnpm 10.6.5:

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm lint
pnpm format:check
pnpm build
pnpm dev
```

The development server binds to 127.0.0.1. Its future /v1 requests proxy to a local backend on port 8080.

Playable scenes, API-generated types, match pages, live viewing, replay/forking and browser interaction tests remain for the API/frontend milestones.
