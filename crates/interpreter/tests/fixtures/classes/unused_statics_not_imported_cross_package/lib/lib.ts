export class Helper {
  constructor(readonly id: number) {}
}

export class Alpha {
  static readonly MAYBE: Helper | null = null;

  static tag(): string {
    return "alpha";
  }
}

export class Top {
  static readonly WIDE: (a: number, b: number, c: number, d: number, e: number) => number = (
    a: number,
    b: number,
    c: number,
    d: number,
    e: number,
  ): number => a + b + c + d + e;

  static apply(f: (a: number, b: number, c: number, d: number, e: number) => number): number {
    return f(1, 2, 3, 4, 5);
  }
}

export class Mid extends Top {}
