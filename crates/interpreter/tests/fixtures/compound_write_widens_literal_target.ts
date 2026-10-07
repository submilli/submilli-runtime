// Arithmetic written back to a variable or field of literal type checks
// against the literal's base type, as in TypeScript: `m += 5` on `m: 1 | 2`
// writes a number, and `m` reads as one afterwards.
let moduleCount: 1 | 2 = 1;

function main(): void {
  let s: "a" = "a";
  s += "b";
  assert(s === "ab");

  let m: 1 | 2 = 1;
  m += 5;
  m *= 2;
  assert(m === 12);

  let z: 1 = 1;
  z = z + 1;
  assert(z === 2);
  z = (z + 1) * 2;
  assert(z === 6);

  let w: 1 = 1;
  w++;
  assert(w === 2);

  let u: 1 | "a" = 1;
  u = u + 1;
  const n: number = u;
  assert(n === 2);

  const p: { c: 0 | 1 } = { c: 0 };
  p.c++;
  p.c += 1;
  assert(p.c === 2);

  const a: (0 | 1)[] = [0];
  a[0] += 1;
  a[0]++;
  const first: number = a[0];
  assert(first === 2);

  moduleCount += 2;
  moduleCount++;
  assert(moduleCount === 4);
}
