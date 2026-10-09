// An optional literal field that sorts before the real tag (`code?` beside
// `kind`) doesn't hide it: a test on `kind` narrows by `kind`.
type S = { code?: 1; kind: "a"; n: number } | { code?: 2; kind: "b"; s: string };
function describe(v: S): string {
  if (v.kind === "a") {
    return `${v.n}`;
  }
  return v.s;
}
function label(v: S): string {
  switch (v.kind) {
    case "a":
      return "A";
    case "b":
      return v.s;
  }
}
function main(): void {
  assert(describe({ kind: "a", n: 1 }) === "1", "narrowed by kind, not code");
  assert(describe({ code: 2, kind: "b", s: "B" }) === "B", "the other member");
  assert(label({ kind: "b", s: "x" }) === "x", "switch narrows by kind and is exhaustive");
}
