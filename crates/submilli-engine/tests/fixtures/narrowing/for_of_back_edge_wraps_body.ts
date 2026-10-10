// Every back edge is a `continue` carrying a narrowing on `x`, so the loop
// fixed point reruns the body and wraps it in a NarrowRegion — the for-of
// desugar must accept the wrapped (non-Block) body.
function run(x: string | null): string {
  const items: Array<string> = ["a", "b"];
  let out = "";
  for (const item of items) {
    if (x === null) {
      out = out + item;
      continue;
    }
    out = out + x + item;
    continue;
  }
  return out;
}

function runIterable(x: string | null): string {
  let out = "";
  for (const ch of "ab") {
    if (x === null) {
      out = out + ch;
      continue;
    }
    out = out + x + ch;
    continue;
  }
  return out;
}

function main(): void {
  assert(run("-") === "-a-b", "narrowed x concatenates");
  assert(run(null) === "ab", "null x skips");
  assert(runIterable("-") === "-a-b", "iterable path: narrowed x");
  assert(runIterable(null) === "ab", "iterable path: null x");
}
