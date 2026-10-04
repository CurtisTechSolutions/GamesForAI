import { BoardScene, object } from "@gfa/game-kit/scene";

const symbols: Record<string, string> = {
  K: "♔",
  Q: "♕",
  R: "♖",
  B: "♗",
  N: "♘",
  P: "♙",
  k: "♚",
  q: "♛",
  r: "♜",
  b: "♝",
  n: "♞",
  p: "♟",
};
export default class ChessScene extends BoardScene {
  private from = "";
  private promotions: string[] = [];
  constructor() {
    super({ key: "chess" });
  }
  private squareName(col: number, row: number) {
    return (
      String.fromCharCode(97 + (this.frame.perspective === 1 ? 7 - col : col)) +
      (this.frame.perspective === 1 ? row + 1 : 8 - row)
    );
  }
  protected paint() {
    this.start(8);
    const pieces = object(object(this.frame.observation).pieces);
    const legal = this.frame.disabled ? [] : this.frame.legalActions;
    if (!legal.some((action) => action.startsWith(this.from))) {
      this.from = "";
      this.promotions = [];
    }
    for (let row = 0; row < 8; row++)
      for (let col = 0; col < 8; col++) {
        const name = this.squareName(col, row),
          piece = pieces[name];
        const destination =
          this.from &&
          legal.some((action) => action.startsWith(this.from + name));
        this.square(
          col,
          row,
          (row + col) % 2 ? 0x41645c : 0xd5dfc5,
          name === this.from || row * 8 + col === this.selected
            ? 0xeeb84e
            : 0x41645c,
          name === this.from || row * 8 + col === this.selected ? 3 : 0,
        );
        if (typeof piece === "string") {
          const white = piece === piece.toUpperCase();
          this.at(
            col,
            row,
            symbols[piece] ?? piece,
            0.76,
            white ? "#ffffff" : "#14252c",
            true,
          ).setStroke(white ? "#1b3136" : "#bcd1bf", 1);
        }
        if (destination)
          this.add.circle(
            this.left + (col + 0.5) * this.cell,
            this.top + (row + 0.5) * this.cell,
            this.cell * 0.13,
            0xeeb84e,
            0.85,
          );
        if (col === 0)
          this.label(
            this.left - 16,
            this.top + (row + 0.5) * this.cell,
            name[1],
            16,
            "#adbaca",
          );
        if (row === 7)
          this.label(
            this.left + (col + 0.5) * this.cell,
            this.top + 8 * this.cell + 15,
            name[0],
            16,
            "#adbaca",
          );
      }
    if (this.promotions.length)
      this.promotions.forEach((action, i) => {
        this.label(
          155 + i * 95,
          579,
          `= ${action.slice(-1).toUpperCase()}`,
          22,
          "#abefd0",
          true,
        )
          .setInteractive({ useHandCursor: true })
          .on("pointerdown", () => this.emit(action));
      });
    else
      this.label(
        300,
        579,
        this.from
          ? "Choose a highlighted destination"
          : "Choose a piece, then its destination",
        16,
        "#adbaca",
      );
  }
  protected choose(col: number, row: number) {
    this.selected = row * 8 + col;
    const name = this.squareName(col, row);
    const choices = this.frame.legalActions.filter(
      (action) =>
        action.startsWith(this.from + name) &&
        action.length >= 4 &&
        action.length <= 5,
    );
    if (this.from && choices.length === 1) {
      this.emit(choices[0]);
      this.from = "";
      this.promotions = [];
    } else if (this.from && choices.length > 1) this.promotions = choices;
    else {
      this.from = this.frame.legalActions.some(
        (action) =>
          /^[a-h][1-8][a-h][1-8]/.test(action) && action.startsWith(name),
      )
        ? name
        : "";
      this.promotions = [];
    }
    this.paint();
  }
  protected key(key: string) {
    const promotion = this.promotions.find((action) =>
      action.endsWith(key.toLowerCase()),
    );
    if (promotion && /^[qrbn]$/i.test(key)) this.emit(promotion);
    else if (key === "Enter" || key === " ")
      this.choose(this.selected % 8, Math.floor(this.selected / 8));
    else if (key === "Escape") {
      this.from = "";
      this.promotions = [];
      this.paint();
    } else this.cursor(key);
  }
}
