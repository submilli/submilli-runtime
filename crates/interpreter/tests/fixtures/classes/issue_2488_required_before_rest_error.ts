// expect-error: override of method `m` is not compatible with the inherited signature
class Base {
  m(x: number, y?: number): number {
    return x + (y ?? 0);
  }
}

class Sub extends Base {
  m(x: number, y: number | undefined, ...zs: number[]): number {
    return x + (y ?? 0) + zs.length;
  }
}

function main(): void {
  const b: Base = new Sub();
  b.m(1);
}
