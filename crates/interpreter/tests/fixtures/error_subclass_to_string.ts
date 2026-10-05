// A user class that extends `Error` (directly, through another user class, or
// through a built-in subclass such as `RangeError`) inherits
// `Error.prototype.toString`: `name: message`, with the separator dropped when
// either side is empty. It holds through an `Error`-typed reference and in
// template interpolation, and a `toString` a class declares still wins, for its
// subclasses too.

class MyError extends Error {}

class Named extends Error {
  code: number;
  constructor(message: string, code: number) {
    super(message);
    this.code = code;
    this.name = "Named";
  }
}

class Leaf extends Named {
  constructor() {
    super("leaf", 2);
  }
}

class OutOfRange extends RangeError {}

class Unnamed extends TypeError {
  constructor(message: string) {
    super(message);
    this.name = "";
  }
}

class FieldNamed extends Error {
  name: string = "FieldNamed";
}

class Custom extends Error {
  toString(): string {
    return "Custom(" + this.message + ")";
  }
}

class CustomChild extends Custom {}

function interpolate(e: Error): string {
  return `${e}`;
}

function viaGeneric<T>(value: T): string {
  return `${value}`;
}

function show(label: string, actual: string, expected: string): void {
  console.log(label, actual);
  assert(actual === expected, label);
}

function main(): void {
  show("direct", new MyError("x").toString(), "Error: x");
  const asError: Error = new MyError("y");
  show("through Error", asError.toString(), "Error: y");
  show("interpolated", `${new MyError("z")}`, "Error: z");
  show("String()", String(new MyError("s")), "Error: s");
  show("empty message", new MyError("").toString(), "Error");
  show("own name", new Named("n", 1).toString(), "Named: n");
  show("inherited name", interpolate(new Leaf()), "Named: leaf");
  show("built-in parent", new OutOfRange("r").toString(), "RangeError: r");
  show("empty name", new Unnamed("only").toString(), "only");
  show("both empty", "[" + new Unnamed("").toString() + "]", "[]");
  show("name from a field initializer", new FieldNamed("f").toString(), "FieldNamed: f");
  show("through a type parameter", viaGeneric<Error>(new MyError("g")), "Error: g");
  try {
    throw new Named("thrown", 3);
  } catch (e) {
    show("caught", String(e), "Named: thrown");
  }
  show("own toString", interpolate(new Custom("c")), "Custom(c)");
  show("inherited toString", new CustomChild("d").toString(), "Custom(d)");

  const errors: Error[] = [new Leaf(), new MyError("m"), new Error("e")];
  let joined = "";
  for (const e of errors) {
    joined += e.toString() + ";";
  }
  show("in an array", joined, "Named: leaf;Error: m;Error: e;");
  show("joined", errors.join("|"), "Named: leaf|Error: m|Error: e");
}
