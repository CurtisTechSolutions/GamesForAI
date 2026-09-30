module.exports = {
  forbidden: [
    {
      name: "site-does-not-import-game-scenes",
      from: { path: "^web/apps/" },
      to: { path: "^web/packages/game-(?!kit/)" },
    },
    {
      name: "game-scenes-only-depend-on-game-kit",
      from: { path: "^web/packages/game-(?!kit/)" },
      to: {
        path: "^web/(apps/|packages/(?!game-kit/|game-[^/]+/))",
      },
    },
    {
      name: "shared-packages-do-not-import-apps",
      from: { path: "^web/packages/" },
      to: { path: "^web/apps/" },
    },
    {
      name: "no-circular-dependencies",
      severity: "error",
      from: {},
      to: { circular: true },
    },
  ],
  options: {
    doNotFollow: { path: "node_modules" },
    tsConfig: { fileName: "tsconfig.json" },
    tsPreCompilationDeps: true,
  },
};
