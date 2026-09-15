export class Calc {
  static readonly ZERO: number = 0;
  static add(a: number, b: number): number {
    return a + b;
  }
}

export class Pure {
  static id(x: number): number {
    return x;
  }
}
