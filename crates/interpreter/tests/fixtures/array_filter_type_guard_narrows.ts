// `filter`, `find` and `findLast` given a type guard return the guarded type,
// as tsc's overloads do: `S[]` and `S | null` rather than the element type.
function isString(x: string | number): x is string {
  return typeof x === "string";
}

type Shape = { kind: "c"; r: number } | { kind: "s"; w: number };

function main(): void {
  const mixed: (string | number)[] = ["a", 1, "b"];
  const named: string[] = mixed.filter(isString);
  assert(named.join(",") === "a,b", "a named guard");

  const inline: string[] = mixed.filter((x): x is string => typeof x === "string");
  assert(inline.join(",") === "a,b", "an inline guard");

  const first: string | null = mixed.find(isString) ?? null;
  assert(first === "a", "find");
  const last: string | null = mixed.findLast(isString) ?? null;
  assert(last === "b", "findLast");

  const maybe: (number | null)[] = [1, null, 3];
  const present = maybe.filter((x): x is number => x !== null);
  assert(present.reduce((sum, n) => sum + n, 0) === 4, "a guard removing `null`");

  const shapes: Shape[] = [{ kind: "c", r: 1 }, { kind: "s", w: 2 }];
  const circles = shapes.filter((s): s is { kind: "c"; r: number } => s.kind === "c");
  assert(circles.length === 1 && circles[0].r === 1, "a union member");

  const unguarded = mixed.filter((x) => typeof x === "string");
  assert(unguarded.length === 2, "a plain predicate keeps the element type");
}
