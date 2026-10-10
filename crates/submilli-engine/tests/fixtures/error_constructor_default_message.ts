// The built-in error constructors take an optional message that defaults to
// `""`, as in JS, so `new Error()` and a bare `super()` compile.
class OutOfRange extends RangeError {}

class Halt extends Error {
  constructor() {
    super();
    this.name = "Halt";
  }
}

function main(): void {
  const plain = new Error();
  assert(plain.message === "" && plain.name === "Error", "Error() has an empty message");
  assert(String(plain) === "Error", "an empty message prints the name alone");
  assert(new TypeError().message === "", "TypeError() has an empty message");
  assert(new RangeError().name === "RangeError", "RangeError() keeps its name");
  assert(new SyntaxError().message === "", "SyntaxError() has an empty message");
  const implicit = new OutOfRange();
  assert(implicit.message === "" && implicit instanceof RangeError, "an implicit constructor passes the default");
  const halt = new Halt();
  assert(halt.message === "" && halt.name === "Halt", "super() passes the default");
  let caught = "";
  try {
    throw new Error();
  } catch (e) {
    caught = e instanceof Error ? "[" + e.message + "]" : "other";
  }
  assert(caught === "[]", "a thrown Error() is caught with its empty message");
}
