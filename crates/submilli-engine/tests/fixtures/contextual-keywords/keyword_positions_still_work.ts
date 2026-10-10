type Alias = number;
type Pair<T> = { first: T; second: T };

function isCircle(
  s: { kind: "circle"; radius: number }
   | { kind: "rect"; height: number; width: number }
): s is { kind: "circle"; radius: number } {
  return s.kind === "circle";
}

function main(): void {
  const type = "runtime";
  assert(type === "runtime");

  const n = 7 as Alias;
  assert(n === 7);

  const p: Pair<number> = { first: 1, second: 2 };
  assert(p.first + p.second === 3);

  const c: { kind: "circle"; radius: number } = { kind: "circle", radius: 5 };
  assert(isCircle(c));

  const of = 4;
  assert(of / 2 === 2);
}
