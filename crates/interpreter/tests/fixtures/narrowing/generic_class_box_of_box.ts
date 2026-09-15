// `Box<Box<string>>` — each hop's field type comes from the declaring class
// with the receiver's type arguments substituted, so a generic class field
// must resolve through the same substitution the access site uses.
class Box<T> {
  constructor(public value: T | null) {}
}

function unwrap(b: Box<Box<string>>): string {
  if (b.value !== null && b.value.value !== null) {
    return b.value.value;
  }
  return "none";
}

function main(): void {
  assert(unwrap(new Box<Box<string>>(new Box<string>("deep"))) === "deep");
  assert(unwrap(new Box<Box<string>>(new Box<string>(null))) === "none");
  assert(unwrap(new Box<Box<string>>(null)) === "none");
}
