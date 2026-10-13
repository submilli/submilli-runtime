// A spread source that writes keeps the context it doesn't fit, `void`
// included: a second pass would read `s` as the `"b"` the first wrote, where
// `k` holds `"a"`. As in tsc, it is rejected.
// expect-error: expected `"b" | undefined`, got `"a"`
function only(k: "b"): string {
  switch (k) {
    case "b":
      return "B";
  }
}

function main(): void {
  let s: "a" | "b" = "a";
  const r: { k: "b"; z: undefined; n: number } = { ...{ k: s, z: void (s = "b") }, n: 1 };
  console.log(only(r.k));
}
