// A callback may declare a parameter wider than the values it is passed, as
// in tsc: parameters are contravariant, so the element type need only be
// assignable to the callback's parameter type.
class Base {
  a: number = 1;
}

class Sub extends Base {
  b: number = 2;
}

function main(): void {
  const doubled = (a: unknown): number => (typeof a === "number" ? a * 2 : -1);
  assert([5].map(doubled).join(",") === "10", "an `unknown` parameter over a `number[]`");

  const size = (a: number | string): number => (typeof a === "number" ? a : a.length);
  assert([5].map(size)[0] === 5, "a union parameter over its member");
  assert(["ab"].map(size)[0] === 2, "and over another member");

  const isBase = (b: Base): boolean => b.a === 1;
  assert([new Sub()].filter(isBase).length === 1, "a base class parameter over subclass elements");

  assert([1, 2].filter((x: number | null) => x !== 2).length === 1, "a nullable parameter");

  let seen = "";
  new Set<number>([1]).forEach((v: unknown) => {
    seen = String(v);
  });
  assert(seen === "1", "a `Set` callback");

  const sum = [1, 2].reduce((acc: number, x: number | string) => acc + (typeof x === "number" ? x : 0), 0);
  assert(sum === 3, "a reducer's element parameter");

  const position = (x: unknown, i: unknown): string => String(i);
  assert([7, 8].map(position).join(",") === "0,1", "an `unknown` index parameter");

  const count = (...xs: unknown[]): number => xs.length;
  assert([4, 5].map(count).join(",") === "3,3", "a rest parameter of `unknown`");
}
