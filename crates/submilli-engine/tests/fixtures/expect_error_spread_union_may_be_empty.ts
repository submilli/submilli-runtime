// A copy of a source that may be null copies nothing then, so it doesn't fit
// a target every object alternative fits (TS2322 in TypeScript).
// expect-error: expected `A | B`
type A = { k: "a"; v: number };
type B = { k: "b"; v: number };

function copy(x: A | B, flag: boolean): A | B {
  return { ...(flag ? x : null) };
}

function main(): void {
  console.log(copy({ k: "a", v: 1 }, false).v);
}
