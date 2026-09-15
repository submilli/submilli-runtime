// Inferred, not annotated: an array-literal element and an object-literal
// field seed their type from the value, so a `void` call has to be caught
// there too or codegen boxes a type with no representation.
// expect-error: `void` cannot be an array element — it has no values
// expect-error: `void` cannot be a field value — it has no values
function nothing(): void {}

function main(): void {
  const a = [nothing()];
  const o = { f: nothing() };
}
