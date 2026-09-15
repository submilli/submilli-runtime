import { Point } from "@test/points";

// Local subclass of an imported class: its own equals body compares the
// flattened field list (inherited x/y + label) behind its own vtable guard.
export class LabeledPoint extends Point {
  label: string;

  constructor(x: number, y: number, label: string) {
    super(x, y);
    this.label = label;
  }
}
