// An object rest declared with `let` can be reassigned, as in JavaScript; one
// declared with `const` cannot (see expect_error_object_rest_const_assign).
function main(): void {
  const source = { a: 1, b: 2, c: 3 };
  let { c, ...rest } = source;
  rest = { a: 7, b: 8 };
  assert(c === 3 && rest.a === 7 && rest.b === 8, "the rest takes a new object");

  const readB = () => rest.b;
  rest = { a: 1, b: 99 };
  assert(readB() === 99, "a closure reads the reassigned rest");
  assert(source.a === 1, "the source is untouched");
}
