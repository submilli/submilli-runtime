/** A point built only through `origin`. */
export class Point {
  private constructor(public x: number) {}

  static origin(): Point {
    return new Point(0);
  }
}

// Neither declares a constructor, so each inherits `Point`'s, private one included.
/** A point with a label. */
export class Labeled extends Point {}

/** A labeled point with a second label. */
export class Labeled2 extends Labeled {}
