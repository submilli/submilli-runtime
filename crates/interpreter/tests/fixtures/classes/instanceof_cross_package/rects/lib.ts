import { Shape } from "@test/shapes";

// Local subclass of an imported class: its vtable's parent link is the
// imported `Shape` singleton, so `instanceof Shape` walks across the package
// boundary.
export class Rect extends Shape {
  w: number;
  h: number;

  constructor(w: number, h: number) {
    super();
    this.w = w;
    this.h = h;
  }

  area(): number {
    return this.w * this.h;
  }
}
