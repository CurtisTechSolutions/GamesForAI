import { BoardScene, object } from "@gfa/game-kit/scene";

export default class SudokuScene extends BoardScene {
  private notes = false;
  constructor() {
    super({ key: "sudoku" });
  }
  protected paint() {
    const data = object(this.frame.observation);
    const n = [4, 6, 9].includes(Number(data.size)) ? Number(data.size) : 9;
    this.start(n);
    const grid = Array.isArray(data.grid) ? data.grid : [],
      givens = Array.isArray(data.givens) ? data.givens : [],
      notes = Array.isArray(data.notes) ? data.notes : [],
      conflicts = Array.isArray(data.conflicts) ? data.conflicts : [];
    for (let i = 0; i < n * n; i++) {
      const row = Math.floor(i / n),
        col = i % n,
        conflict = conflicts.includes(i);
      this.square(
        col,
        row,
        conflict ? 0x583a3b : i === this.selected ? 0x305249 : 0x1c2d37,
        i === this.selected ? 0xabefd0 : 0x455665,
        i === this.selected ? 3 : 1,
      );
      if (Number(grid[i]) > 0)
        this.at(
          col,
          row,
          String(grid[i]),
          0.52,
          conflict ? "#ffc1b5" : givens[i] ? "#ffffff" : "#abefd0",
          Boolean(givens[i]),
        );
      else
        for (let digit = 1; digit <= n; digit++)
          if ((Number(notes[i]) & (1 << (digit - 1))) !== 0) {
            const width = Math.ceil(Math.sqrt(n));
            this.label(
              this.left +
                col * this.cell +
                ((((digit - 1) % width) + 0.5) * this.cell) / width,
              this.top +
                row * this.cell +
                ((Math.floor((digit - 1) / width) + 0.5) * this.cell) / width,
              String(digit),
              (this.cell / width) * 0.56,
              "#c2d0db",
            );
          }
    }
    const boxWidth = n === 4 ? 2 : 3,
      boxHeight = n === 9 ? 3 : 2;
    const lines = this.add.graphics().lineStyle(3, 0x9eafa8);
    for (let i = 0; i <= n; i++) {
      if (i % boxWidth === 0)
        lines.lineBetween(
          this.left + i * this.cell,
          this.top,
          this.left + i * this.cell,
          this.top + n * this.cell,
        );
      if (i % boxHeight === 0)
        lines.lineBetween(
          this.left,
          this.top + i * this.cell,
          this.left + n * this.cell,
          this.top + i * this.cell,
        );
    }
    this.label(
      300,
      579,
      this.notes
        ? "Notes on · N to switch to digits"
        : "Digits · N for notes · Backspace to erase",
      17,
      this.notes ? "#f4cd88" : "#adbaca",
    );
  }
  protected choose(col: number, row: number) {
    this.selected = row * this.columns + col;
    this.paint();
  }
  protected key(key: string) {
    if (key.toLowerCase() === "n") {
      this.notes = !this.notes;
      this.events.emit("board-notes", this.notes);
      this.paint();
      return;
    }
    const prefix = `r${Math.floor(this.selected / this.columns) + 1}c${(this.selected % this.columns) + 1}`;
    if (key === "Backspace" || key === "Delete" || key === "0")
      this.emit(prefix + "=0");
    else if (/^[1-9]$/.test(key)) {
      if (!this.notes) this.emit(prefix + "=" + key);
      else {
        const action = this.frame.legalActions.find(
          (value) =>
            value === prefix + "+" + key || value === prefix + "-" + key,
        );
        if (action) this.emit(action);
      }
    } else this.cursor(key);
  }
}
