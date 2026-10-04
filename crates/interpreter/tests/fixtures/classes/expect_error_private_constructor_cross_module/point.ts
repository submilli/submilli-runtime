/** A point built only through `origin`. */
export class Point {
  private constructor(public x: number) {}

  static origin(): Point {
    return new Point(0);
  }
}

// Declares no constructor, so it inherits `Point`'s, private one included.
/** A point with a label. */
export class Labeled extends Point {
  label(): string {
    return "p";
  }
}
