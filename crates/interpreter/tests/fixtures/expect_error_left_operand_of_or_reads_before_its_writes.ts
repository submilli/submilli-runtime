// A context the left operand of `||` doesn't fit isn't retried when the
// operand writes: its first pass has already narrowed `s` to `"b"`, so a
// second would read `k` as `"b"` where it holds `"a"`. As in tsc, it is
// rejected.
// expect-error: expected `"b"`, got `"a"`
function only(k: "b"): string {
  switch (k) {
    case "b":
      return "B";
  }
}

function main(): void {
  let s: "a" | "b" = "a";
  const r: { k: "b"; z: string } = { k: s, z: (s = "b") } || { k: "b", z: "" };
  console.log(only(r.k));
}
