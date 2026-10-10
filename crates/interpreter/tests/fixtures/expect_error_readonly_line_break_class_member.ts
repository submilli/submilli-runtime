// expect-error: expected `:` and a type for the class field
// A modifier at the end of a line doesn't modify the member on the next line:
// the line break ends `readonly` as a member of its own, as tsc reads it.
class C {
  readonly
  a: number = 1;
}

function main(): void {}
