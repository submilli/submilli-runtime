// A literal or conditional spread into an object literal takes that literal's
// context, as in tsc, so its fields keep the literal types the expected type
// names. Fields written after the spread may supply the rest.
type Shape = { kind: "circle"; r: number } | { kind: "square"; side: number };
type Named = { kind: "a" | "b"; n: number };

function pick(flag: boolean): Shape {
  return { ...(flag ? { kind: "circle", r: 1 } : { kind: "square", side: 2 }) };
}

function named(flag: boolean): Named {
  return { ...{ kind: flag ? "a" : "b" }, n: 1 };
}

function main(): void {
  assert(JSON.stringify(pick(true)) === '{"kind":"circle","r":1}', "the circle branch");
  assert(JSON.stringify(pick(false)) === '{"kind":"square","side":2}', "the square branch");
  assert(named(false).kind === "b" && named(false).n === 1, "a field after the spread");
  const free = { ...(Math.random() < 2 ? { k: "x" } : { k: "y" }) };
  free.k = "z";
  assert(free.k === "z", "without a context the fields widen");
}
