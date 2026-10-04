import { BoardScene, object } from "@gfa/game-kit/scene";

export default class TicTacToeScene extends BoardScene {
  constructor() {
    super({ key: "tictactoe" });
  }
  protected paint() {
    this.start(3);
    const data = object(this.frame.observation);
    const board = Array.isArray(data.board) ? data.board : [];
    for (let i = 0; i < 9; i++) {
      const col = i % 3,
        row = Math.floor(i / 3);
      const allowed =
        !this.frame.disabled &&
        this.frame.legalActions.includes(`r${row + 1}c${col + 1}`);
      this.square(
        col,
        row,
        allowed ? 0x223a38 : 0x192831,
        this.selected === i ? 0xabefd0 : 0x30404d,
        this.selected === i ? 3 : 1,
      );
      this.at(
        col,
        row,
        board[i] === 0 ? "×" : board[i] === 1 ? "○" : "",
        0.72,
        board[i] === 0 ? "#abefd0" : "#f4cd88",
      );
      this.label(
        this.left + col * this.cell + 24,
        this.top + row * this.cell + 24,
        `${row + 1},${col + 1}`,
        15,
        "#adbaca",
      );
    }
  }
  protected choose(col: number, row: number) {
    this.selected = row * 3 + col;
    this.paint();
    this.emit(`r${row + 1}c${col + 1}`);
  }
  protected key(key: string) {
    if (key === "Enter" || key === " ")
      this.choose(this.selected % 3, Math.floor(this.selected / 3));
    else this.cursor(key);
  }
}
