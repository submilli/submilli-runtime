function joinNonNull(items: Array<string | null>): string {
  let out = "";
  for (const item of items) {
    if (item === null) {
      continue;
    }
    out = out + item;
  }
  return out;
}

function main(): void {
  assert(joinNonNull(["a", null, "b"]) === "ab", "loop var narrows after continue");
  assert(joinNonNull([null, null]) === "", "all null");
}
