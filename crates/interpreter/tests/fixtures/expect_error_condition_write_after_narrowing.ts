// A narrowing is a fact about the value read; a write later in the same
// condition replaces that value before the branch runs. TypeScript rejects
// this too (TS18047).
// expect-error: cannot read field `length` on `string | null`

function takesNull(v: null): boolean {
  return v === null;
}

function main(): void {
  let x: string | null = "a";
  if (x !== null && takesNull(x = null)) {
    console.log(`${x.length}`);
  }
}
