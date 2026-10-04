/** Shared drawing/input primitives. Games render observations; legality comes from the server. */
import Phaser from "phaser";

export interface SceneFrame {
  observation: unknown;
  perspective: number;
  legalActions: string[];
  disabled: boolean;
}

export function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

export abstract class BoardScene extends Phaser.Scene {
  onReady?: () => void;
  protected frame: SceneFrame = {
    observation: {},
    perspective: 0,
    legalActions: [],
    disabled: true,
  };
  protected columns = 1;
  protected rows = 1;
  protected size = 520;
  protected left = 40;
  protected top = 30;
  protected cell = 520;
  protected selected = 0;

  create() {
    this.input.on("pointerdown", (pointer: Phaser.Input.Pointer) => {
      const col = Math.floor((pointer.x - this.left) / this.cell);
      const row = Math.floor((pointer.y - this.top) / this.cell);
      if (col >= 0 && col < this.columns && row >= 0 && row < this.rows) {
        this.choose(col, row);
      }
    });
    this.events.on("board-key", (key: string) => this.key(key));
    this.onReady?.();
  }

  display(frame: SceneFrame) {
    this.frame = frame;
    this.paint();
  }
  protected abstract paint(): void;
  protected abstract choose(col: number, row: number): void;
  protected abstract key(key: string): void;

  protected start(columns: number, rows = columns) {
    this.children.removeAll(true);
    this.columns = columns;
    this.rows = rows;
    this.cell = this.size / Math.max(columns, rows);
    this.left = (600 - columns * this.cell) / 2;
    this.top = (570 - rows * this.cell) / 2;
  }
  protected emit(action: string) {
    if (!this.frame.disabled && this.frame.legalActions.includes(action)) {
      this.events.emit("board-action", action);
    }
  }
  protected label(
    x: number,
    y: number,
    text: string,
    size = 24,
    color = "#e9eef5",
    bold = false,
  ) {
    return this.add
      .text(x, y, text, {
        fontFamily: "Arial, sans-serif",
        fontSize: size,
        color,
        fontStyle: bold ? "bold" : "normal",
      })
      .setOrigin(0.5);
  }
  protected square(
    col: number,
    row: number,
    fill: number,
    stroke = 0x30404d,
    width = 1,
  ) {
    return this.add
      .rectangle(
        this.left + (col + 0.5) * this.cell,
        this.top + (row + 0.5) * this.cell,
        this.cell - 2,
        this.cell - 2,
        fill,
      )
      .setStrokeStyle(width, stroke);
  }
  protected at(
    col: number,
    row: number,
    text: string,
    scale = 0.55,
    color = "#e9eef5",
    bold = false,
  ) {
    return this.label(
      this.left + (col + 0.5) * this.cell,
      this.top + (row + 0.5) * this.cell,
      text,
      this.cell * scale,
      color,
      bold,
    );
  }
  protected cursor(key: string) {
    const row = Math.floor(this.selected / this.columns),
      col = this.selected % this.columns;
    if (key === "ArrowLeft")
      this.selected = row * this.columns + Math.max(0, col - 1);
    if (key === "ArrowRight")
      this.selected = row * this.columns + Math.min(this.columns - 1, col + 1);
    if (key === "ArrowUp")
      this.selected = Math.max(0, row - 1) * this.columns + col;
    if (key === "ArrowDown")
      this.selected = Math.min(this.rows - 1, row + 1) * this.columns + col;
    this.paint();
  }
}
