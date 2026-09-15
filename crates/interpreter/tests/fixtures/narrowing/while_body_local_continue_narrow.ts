function joinNonNull(items: Array<string | null>): string {
  let out = "";
  let i = 0;
  while (i < items.length) {
    const s = items[i];
    i = i + 1;
    if (s === null) {
      continue;
    }
    out = out + s;
  }
  return out;
}

function main(): void {
  assert(joinNonNull(["a", null, "b"]) === "ab", "skips nulls");
  assert(joinNonNull([null]) === "", "all null");
}
