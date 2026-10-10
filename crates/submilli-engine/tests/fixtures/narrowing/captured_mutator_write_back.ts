// The rewrite the captured-mutator hint names has to compile. Stability rule 5
// refuses the narrowing for any binding a closure reassigns — including inside
// that closure — so re-narrowing in place does not help and the hint must not
// suggest it. Reading into a `const`, narrowing that, and writing back does.

export function main(): string {
  let v: number | null = 1;
  const bump = (): void => {
    const cur = v;
    if (cur !== null) {
      v = cur + 1;
    }
  };
  bump();
  bump();

  let s: string | null = "a";
  const append = (): void => {
    const cur = s;
    if (cur !== null) {
      s = cur + "!";
    }
  };
  append();

  const outV = v;
  const outS = s;
  assert(outV !== null && outV === 3, "the write-back form compiles and runs");
  assert(outS !== null && outS === "a!", "and for a string target too");
  return "ok";
}
