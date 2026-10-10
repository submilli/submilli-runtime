// expect-error: non-exhaustive `switch`
// expect-error-count: 1
// The exit where no case matched carries only the unmatched members, so `k`
// is `"a" | "b"` after the switch and indexing `o` with it is fine. The
// missing cases are still an error (spec §1.5).
type Key = "a" | "b" | "c";

function pickField(o: { a: number; b: number }, k: Key): number {
  switch (k) {
    case "c":
      k = "a";
      break;
  }
  return o[k];
}

function main(): void {
  pickField({ a: 1, b: 2 }, "c");
}
