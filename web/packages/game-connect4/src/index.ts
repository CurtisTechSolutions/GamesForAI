import { BoardScene, object } from "@gfa/game-kit/scene";

export default class ConnectFourScene extends BoardScene {
  constructor() {
    super({ key: "connect4" });
  }
  protected paint() {
    this.start(7, 6);
    const data = object(this.frame.observation);
    const rows = Array.isArray(data.rows) ? data.rows : [];
    for (let col = 0; col < 7; col++) {
      const allowed =
        !this.frame.disabled &&
        this.frame.legalActions.includes(String(col + 1));
      this.label(
        this.left + (col + 0.5) * this.cell,
        this.top - 22,
        String(col + 1),
        22,
        allowed ? "#abefd0" : "#adbaca",
        true,
      );
      for (let row = 0; row < 6; row++) {
        this.square(col, row, col === this.selected ? 0x294954 : 0x1f3545);
        const value = Array.isArray(rows[row]) ? rows[row][col] : null;
        const x = this.left + (col + 0.5) * this.cell,
          y = this.top + (row + 0.5) * this.cell;
        this.add.circle(
          x,
          y,
          this.cell * 0.38,
          value === 0 ? 0xff9688 : value === 1 ? 0xf4cd88 : 0x101820,
        );
        if (value === 0 || value === 1)
          this.label(x, y, value === 0 ? "R" : "Y", 20, "#15232a", true);
      }
    }
  }
  protected choose(col: number) {
    this.selected = col;
    this.paint();
    this.emit(String(col + 1));
  }
  protected key(key: string) {
    if (/^[1-7]$/.test(key)) this.choose(Number(key) - 1);
    else if (key === "Enter" || key === " ") this.choose(this.selected);
    else {
      if (key === "ArrowLeft") this.selected = Math.max(0, this.selected - 1);
      if (key === "ArrowRight") this.selected = Math.min(6, this.selected + 1);
      this.paint();
    }
  }
}
