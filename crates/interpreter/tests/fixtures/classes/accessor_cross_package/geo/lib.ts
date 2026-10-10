export interface Sized {
  size: number;
}

export class Circle implements Sized {
  private r: number;
  constructor(r: number) {
    this.r = r;
  }

  // Read-write accessor: `size` is the diameter, backed by the radius.
  get size(): number {
    return this.r * 2;
  }
  set size(d: number) {
    this.r = d / 2;
  }
}
