// A spread field that may be absent can't fill a field the target requires
// (TS2322 in TypeScript): an optional spread field, or one that only some
// alternatives of a conditional spread have.
// expect-error: spread field `a` may be absent, but the target requires it
// expect-error: give `a` a value after the spread
interface Required {
  a: number;
}

function pick(b: boolean): boolean {
  return b;
}

function main(): void {
  const r: Required = { ...(pick(false) ? { a: 1 } : { b: 2 }) };
  console.log(r.a);
}
