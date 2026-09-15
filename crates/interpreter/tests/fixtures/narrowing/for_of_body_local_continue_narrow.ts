function joinNonNull(items: Array<string | null>): string {
  let out = "";
  for (const item of items) {
    const s = item;
    if (s === null) {
      continue;
    }
    out = out + s;
  }
  return out;
}

function main(): void {
  assert(joinNonNull(["a", null, "b"]) === "ab", "skips nulls");
  assert(joinNonNull([null, null]) === "", "all null");
  assert(joinNonNull([]) === "", "empty array");
}
