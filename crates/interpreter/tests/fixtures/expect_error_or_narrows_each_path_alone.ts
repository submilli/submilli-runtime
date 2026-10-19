// expect-error: expected `"a"`, got `"" | "a"`
// `s || x` is true when `s` is, or when `s` is "" and `x` is truthy. `x: 0`
// never is, but as in TypeScript that impossibility belongs to `x`: `s` still
// joins both ways and reads `"" | "a"`.
function f(s: "" | "a", x: 0): void {
  if (s || x) {
    const t: "a" = s;
  }
}

function main(): void {}
