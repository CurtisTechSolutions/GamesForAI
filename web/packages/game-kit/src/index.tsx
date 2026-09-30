import { useEffect, useRef } from "react";
import Phaser from "phaser";

/** Scenes render server observations; they never compute legality or outcomes. */
export interface GameScenePlugin {
  gameId: string;
  createScene: () => Phaser.Scene;
  render: (scene: Phaser.Scene, observation: unknown, perspective: number) => void;
  onInput: (scene: Phaser.Scene, callback: (action: unknown) => void) => () => void;
}

const plugins = new Map<string, GameScenePlugin>();

export function registerGameScene(plugin: GameScenePlugin) {
  if (plugins.has(plugin.gameId)) throw new Error(`Duplicate scene: ${plugin.gameId}`);
  plugins.set(plugin.gameId, plugin);
}

export function getGameScene(gameId: string) {
  return plugins.get(gameId);
}

/** Phaser owns a board canvas; React owns navigation, panels and match data. */
export function BoardHost({
  plugin,
  observation,
  perspective,
  onAction,
}: {
  plugin: GameScenePlugin;
  observation: unknown;
  perspective: number;
  onAction: (action: unknown) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const scene = useRef<Phaser.Scene | null>(null);
  const latest = useRef({ observation, perspective, onAction });
  latest.current = { observation, perspective, onAction };

  useEffect(() => {
    if (!host.current) return;
    const board = plugin.createScene();
    let dispose: (() => void) | undefined;
    board.events.once(Phaser.Scenes.Events.CREATE, () => {
      scene.current = board;
      plugin.render(board, latest.current.observation, latest.current.perspective);
      dispose = plugin.onInput(board, (action) => latest.current.onAction(action));
    });
    const game = new Phaser.Game({
      type: Phaser.AUTO,
      parent: host.current,
      width: 600,
      height: 600,
      backgroundColor: "#182331",
      scale: { mode: Phaser.Scale.FIT, autoCenter: Phaser.Scale.CENTER_BOTH },
      scene: board,
    });
    return () => {
      dispose?.();
      scene.current = null;
      game.destroy(true);
    };
  }, [plugin]);

  useEffect(() => {
    if (scene.current) plugin.render(scene.current, observation, perspective);
  }, [plugin, observation, perspective]);

  return <div ref={host} aria-label="Game board" />;
}

/** Every engine is playable via its authoritative legal-action list. */
export function TextBoard({
  text,
  legalActions,
  onAction,
  disabled = false,
}: {
  text: string;
  legalActions: string[];
  onAction: (action: string) => void;
  disabled?: boolean;
}) {
  return (
    <section aria-label="Accessible game controls">
      <pre aria-live="polite">{text}</pre>
      <label>
        Legal move
        <select
          value=""
          disabled={disabled || legalActions.length === 0}
          onChange={(event) => onAction(event.target.value)}
        >
          <option value="">Choose a move</option>
          {legalActions.map((action) => <option key={action} value={action}>{action}</option>)}
        </select>
      </label>
    </section>
  );
}
