// A member typing the field with a type parameter stops it being a
// discriminant, as in TypeScript, so the test narrows nothing.
// expect-error: field `b` does not exist on all members
// expect-error-count: 1
function firstOf<T>(x: { a: 0; b: string } | { a: T; c: number }): string {
  if (x.a === 0) {
    return x.b;
  }
  return "";
}

function main(): void {
  console.log(firstOf<number>({ a: 0, b: "b" }));
}
