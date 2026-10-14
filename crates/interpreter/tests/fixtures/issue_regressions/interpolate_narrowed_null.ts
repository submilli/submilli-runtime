// A value narrowed to null interpolates as "null", whatever union it was
// narrowed from.
function describe(n: number | null): string {
  if (n === null) {
    return `${n}`;
  }
  return `${n + 1}`;
}

function label(s: string | null): string {
  return s === null ? `none: ${s}` : `some: ${s}`;
}

function main(): void {
  assert(describe(null) === "null", "a number or null narrowed to null");
  assert(describe(1) === "2", "the number side");
  assert(label(null) === "none: null", "a string or null narrowed to null");
  assert(label("a") === "some: a", "the string side");
  const opaque: unknown = Math.random() > 2 ? 1 : null;
  if (opaque === null) {
    assert(`${opaque}` === "null", "unknown narrowed to null");
  }
}
