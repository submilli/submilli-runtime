// A literal type that isn't fresh (annotated) stays when it is inferred for a
// type parameter, and a fresh one passed for a type parameter that isn't the
// whole result widens unless the expected result asks for it, as in tsc.
// expect-error: expected `"items"`, got `"zz"`
// expect-error: expected `() => "items"`, got `() => string`
// expect-error-count: 2
function box<T>(v: T): { v: T } {
  return { v: v };
}

function later<T>(x: T): () => T {
  return () => x;
}

function main(): void {
  const annotated: "items" = "items";
  const b = box(annotated);
  b.v = "zz";
  const label = "items";
  const asked: () => "items" = later(label);
  const f = later(label);
  const g: () => "items" = f;
  console.log(b.v, asked(), g());
}
