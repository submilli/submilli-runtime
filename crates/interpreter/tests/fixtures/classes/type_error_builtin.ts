class MissingValueError extends TypeError {
  constructor(message: string) {
    super(message);
    this.name = "MissingValueError";
  }
}

function main(): void {
  // Constructible, with the right name and message.
  const e = new TypeError("wrong type");
  assert(e.message === "wrong type", "message field");
  assert(e.name === "TypeError", "name field");
  assert(e instanceof TypeError, "instanceof own class");
  assert(e instanceof Error, "instanceof parent");
  assert(Error.isError(e), "Error.isError sees the subclass");
  assert(e.toString() === "TypeError: wrong type", "toString");

  // Sibling built-in subclasses are distinct (checked through the shared
  // Error type — direct sibling instanceof is a static always-false error).
  const range: Error = new RangeError("out of range");
  assert(!(range instanceof TypeError), "RangeError is not TypeError");
  const asError: Error = e;
  assert(!(asError instanceof RangeError), "TypeError is not RangeError");
  const base = new Error("plain");
  assert(!(base instanceof TypeError), "base Error is not TypeError");

  // Typed catch filters: TypeError arm binds a thrown TypeError.
  let caught = "";
  try {
    throw new TypeError("thrown");
  } catch (e: TypeError) {
    caught = e.name + ":" + e.message;
  }
  assert(caught === "TypeError:thrown", "typed catch binds TypeError");

  // A base Error skips the TypeError arm and lands in the catch-all.
  let arm = "";
  try {
    throw new Error("base");
  } catch (e: TypeError) {
    arm = "type";
  } catch (e) {
    arm = "base:" + e.name;
  }
  assert(arm === "base:Error", "base Error skips the TypeError arm");

  // A failed non-null assertion throws a TypeError.
  const absent: string | null = null;
  let asserted = "";
  try {
    const x = absent!;
    asserted = x;
  } catch (e: TypeError) {
    asserted = e.name + ":" + e.message;
  }
  assert(
    asserted === "TypeError:non-null assertion failed: value is null",
    "x! on null is TypeError",
  );

  // User subclasses of TypeError chain through it to Error.
  const custom = new MissingValueError("field absent");
  assert(custom instanceof MissingValueError, "instanceof own class");
  assert(custom instanceof TypeError, "instanceof TypeError parent");
  assert(custom instanceof Error, "instanceof Error root");
  assert(custom.name === "MissingValueError", "subclass name");
}
