import type { BoardScene } from "./scene";

type SceneModule = { default: new () => BoardScene };
/** Add one lazy registration per game. Unregistered games use the text board. */
const scenes: Record<string, () => Promise<SceneModule>> = {
  tictactoe: () => import("../../game-tictactoe/src/index"),
  connect4: () => import("../../game-connect4/src/index"),
  chess: () => import("../../game-chess/src/index"),
  sudoku: () => import("../../game-sudoku/src/index"),
};
export const hasGameScene = (id: string) => Object.hasOwn(scenes, id);
export const loadGameScene = (id: string) => scenes[id]();
