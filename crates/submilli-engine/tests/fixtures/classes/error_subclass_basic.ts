// `Error` is a real (host-implemented) class: user classes extend it, set
// `name` in their constructor, and narrow with `instanceof`.
class MyError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "MyError";
  }
}

class OtherError extends Error {
  code: number;
  constructor(message: string, code: number) {
    super(message);
    this.name = "OtherError";
    this.code = code;
  }
}

function main(): void {
  const e = new MyError("boom");
  assert(e.message === "boom", "subclass message round-trips through super()");
  assert(e.name === "MyError", "subclass ctor overrides name");
  assert(e instanceof MyError, "instance of its own class");
  assert(e instanceof Error, "instance of the Error base");

  const base = new Error("plain");
  assert(base instanceof Error, "base instance of Error");
  assert(!(base instanceof MyError), "base is not a subclass instance");

  const o = new OtherError("bad", 42);
  assert(o.code === 42, "subclass adds its own fields after the inherited ones");
  assert(o.message === "bad", "inherited field readable on subclass");
}
