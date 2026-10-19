// A fresh object literal joined with other object types reads a field only
// they declare as `undefined`, as tsc normalizes it: `(options || {}).a` is
// `string | undefined`.
function first(options?: { a: string; b: number }): string | undefined {
  const dotted: string | undefined = (options || {}).a;
  const bracketed: string | undefined = (options ?? {})["a"];
  const chosen = (options ? options : { b: 0 }).a;
  return dotted === bracketed && bracketed === chosen ? dotted : "differ";
}

type Inner = { a: number };

// Only a falsy `o` reaches the `||`, so the literal is told apart.
function guarded(o: { inner: Inner | undefined } | undefined): number | undefined {
  return ((o && o.inner) || {}).a;
}

// Two declared operands beside literals of a shape neither declares.
function chained(
  x: { a: number; c: string } | undefined,
  y: { a: number; c: string } | undefined,
  flag: boolean,
): string | undefined {
  return (x ?? y ?? (flag ? { d: 1 } : { d: 2 })).c;
}

function main(): void {
  assert(first() === undefined, "the literal lacks the field");
  assert(first({ a: "s", b: 1 }) === "s", "the object holds the field");
  assert(guarded(undefined) === undefined, "a falsy `o` reads the literal");
  assert(guarded({ inner: { a: 3 } }) === 3, "the inner object holds the field");
  assert(chained(undefined, undefined, true) === undefined, "the literals lack the field");
  assert(chained(undefined, { a: 1, c: "y" }, true) === "y", "the second operand holds it");
}
