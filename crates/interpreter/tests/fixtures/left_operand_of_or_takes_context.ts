// The left side of `||` and `??` takes the expression's context, as in tsc,
// so a tuple, a literal union or a callback there types by it. A branch that
// falls through to the right side (`undefined`, `null`, `false`) still fits.
function main(): void {
  const n: string = "q";
  const flag = Math.random() < 2;
  const viaNullish: [string, number] = (flag ? [n, 1] : undefined) ?? [n, 2];
  assert(viaNullish[1] === 1, "a tuple on the left of ??");

  const viaOr: [string, number] = (flag ? [n, 3] : null) || [n, 4];
  assert(viaOr[1] === 3, "a tuple on the left of ||");

  const kind: "a" | "b" = (flag ? "a" : undefined) ?? "b";
  assert(kind === "a", "a literal union");

  const step: (x: number) => number = (flag ? (x) => x + 1 : undefined) ?? ((x) => x);
  assert(step(1) === 2, "a callback's parameter");

  const shape: { k: "on" } = (flag ? { k: "on" } : false) || { k: "on" };
  assert(shape.k === "on", "an object literal on the left of ||");
}
