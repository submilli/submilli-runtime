// Each alias holds two copies of the previous one, so A13 has 40,958 parts,
// within the compiler limit of 65,536 (A14 has 81,918).
type A0 = { v: number };
type A1 = { a: A0; b: A0 };
type A2 = { a: A1; b: A1 };
type A3 = { a: A2; b: A2 };
type A4 = { a: A3; b: A3 };
type A5 = { a: A4; b: A4 };
type A6 = { a: A5; b: A5 };
type A7 = { a: A6; b: A6 };
type A8 = { a: A7; b: A7 };
type A9 = { a: A8; b: A8 };
type A10 = { a: A9; b: A9 };
type A11 = { a: A10; b: A10 };
type A12 = { a: A11; b: A11 };
type A13 = { a: A12; b: A12 };
function first(x: A13 | null): number {
  return x === null ? 0 : x.a.b.a.b.a.b.a.b.a.b.a.b.a.v;
}
export function main(): number {
  const result = first(null);
  if (result !== 0) throw new Error("expected 0");
  return result;
}
