// expect-error: expected `"b"`, got `"a"`
function main(): void { const x: Exclude<"a" | "b", "a"> = "a"; }
