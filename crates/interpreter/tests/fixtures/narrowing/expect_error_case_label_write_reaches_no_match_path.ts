// expect-error: expected `"b"`, got `"a"`
// expect-error-count: 1
// When no case matches, every label has run, so one that writes the
// discriminant leaves what the cases ruled out a fact about the old value.
function set(x: "a"): "a" {
  return "a";
}
function f(t: "a" | "b" | number): string {
  let x: "a" | "b" | number = t;
  switch (x) {
    case set((x = "a")):
      return "matched";
  }
  if (typeof x === "number") return "number";
  const b: "b" = x;
  return b;
}
function main(): void {
  f("b");
}
