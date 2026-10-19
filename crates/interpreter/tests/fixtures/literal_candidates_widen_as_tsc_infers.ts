// Literal candidates widen where tsc widens them: a seed's literal union when a
// callback fixes the type parameter through its parameter, and a callback's
// literal-union result when a later argument gives the base type.
function iterate<T>(step: (x: T) => T, seed: T): T {
  return step(seed);
}

function fold<A>(o: { step: (acc: A, x: string) => A }, init: A, xs: string[]): A {
  let acc = init;
  for (const x of xs) {
    acc = o.step(acc, x);
  }
  return acc;
}

function use<U>(o: { use: (t: number) => U }, pair: { u: U }): U {
  return o.use(pair.u === null ? 0 : 1) ?? pair.u;
}

function main(): void {
  const flag = Math.random() < 2;
  let next = iterate((x) => x + 1, flag ? 10 : 20);
  assert(next === 11, `iterate: ${next}`);
  next = 99;
  const folded = fold({ step: (acc, x) => acc + x.length }, flag ? 10 : 20, ["ab", "c"]);
  assert(folded === 13, `fold: ${folded}`);
  const used = use({ use: (t: number) => (t > 0 ? 1 : null) }, { u: 5 });
  assert(used === 1, `use: ${used}`);
}
