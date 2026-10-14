// A method may override an inherited optional method declared without a body,
// as in tsc. Reads through the ancestor's type see the override, and the
// inherited member stays an assignable field.
class Base {
  f?(x: number): number;
  run(): string {
    return this.f ? `${this.f(2)}` : "none";
  }
}
class Mid extends Base {
  f(x: number, y?: number): number {
    return x * 10 + (y ?? 0);
  }
}
class Leaf extends Mid {
  f(x: number, y?: number): number {
    return x * 100;
  }
}
class Box<T> {
  get?(): T;
}
class NumberBox extends Box<number> {
  get(): number {
    return 7;
  }
}
// A middle class that redeclares the optional method leaves the slot alone.
class Redeclared extends Base {
  f?(x: number): number;
}
class Filled extends Redeclared {
  f(x: number): number {
    return x + 1;
  }
}
interface HasF {
  f?(x: number): number;
}

function main(): void {
  const items: Base[] = [new Base(), new Mid(), new Leaf()];
  assert(items.map((it) => `${it.run()} ${it.f?.(3)}`).join("; ") === "none undefined; 20 30; 200 300");
  assert(new Mid().f(1, 2) === 12);
  const h: HasF = new Leaf();
  assert(h.f?.(4) === 400);
  const box: Box<number> = new NumberBox();
  assert(box.get?.() === 7);

  const filled = new Filled();
  const redeclared: Redeclared = filled;
  assert(`${filled.f(1)} ${redeclared.f?.(1)} ${filled.run()}` === "2 2 3");

  const mid = new Mid();
  const base: Base = mid;
  base.f = (x: number): number => -x;
  assert(mid.f(5) === -5 && mid.run() === "-2");
}
