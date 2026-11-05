// expect-error: unknown type `Row`
// expect-error-count: 2
// A local type is not visible after its block ends, as an annotation or as
// the class an `instanceof` names.

function main(): void {
  {
    interface Row { a: number; }
    const inside: Row = { a: 1 };
    assert(inside.a === 1, "in scope");
  }
  const outside: Row = { a: 2 };
  const tested = (outside as unknown) instanceof Row;
}
