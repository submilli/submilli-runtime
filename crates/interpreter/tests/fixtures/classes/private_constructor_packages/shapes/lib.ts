/** A point built through `at`. */
export class Point {
  private constructor(public x: number) {}

  static at(x: number): Point {
    return new Point(x);
  }
}

// Declared in `Point`'s module, so it may extend `Point` and call its
// constructor; its own constructor is public.
/** A point that describes itself. */
export class Tagged extends Point {
  constructor(x: number) {
    super(x);
  }

  tag(): string {
    return "x=" + String(this.x);
  }
}
