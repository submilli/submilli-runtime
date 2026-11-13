// A union of number literals returned where the result may be boxed
// (`main`, a generic function, a method, or `JSON.stringify`) keeps its
// unboxed `f64` representation and boxes/formats cleanly when needed.

type Code = 1 | 2;
type NestedCode = (0 | 1) | (2 | 3);
type Sign = -1 | 1;
type Indent = 2 | 4;
type Pad = "  " | "\t";

function indented(value: unknown, space: Indent): string {
  return JSON.stringify(value, null, space);
}

function padded(value: unknown, space: Pad): string {
  return JSON.stringify(value, null, space);
}

function templated(value: unknown, space: `-${string}`): string {
  return JSON.stringify(value, null, space);
}

function pickCode(flag: boolean): Code {
  return flag ? 1 : 2;
}

function pickFloat(flag: boolean): 1.5 | 2.5 {
  return flag ? 1.5 : 2.5;
}

function pickNested(n: number): NestedCode {
  if (n === 0) return 0;
  if (n === 1) return 1;
  if (n === 2) return 2;
  return 3;
}

function pickBool(flag: boolean): true | false {
  return flag ? true : false;
}

function genericTwo<T>(_x: T): 1 | 2 {
  return 2;
}

function genericQuad<T>(x: T, flag: boolean): 0 | 1 | 2 | 3 {
  return x ? (flag ? 3 : 1) : 0;
}

function genericSign<T>(x: T): Sign {
  return x ? 1 : -1;
}

function genericOrOne<T>(x: T, useArg: boolean): T | 1 {
  return useArg ? x : 1;
}

function genericOptional<T>(x: T, present: boolean): 1 | 2 | undefined {
  return present ? (x ? 2 : 1) : undefined;
}

function genericMixed<T>(x: T, num: boolean): 1 | "a" {
  return num ? 1 : (x ? "a" : "a");
}

function genericStrings<T>(x: T): "a" | "b" {
  return x ? "a" : "b";
}

function genericBool<T>(x: T): true | false {
  return Boolean(x);
}

function id<T>(x: T): T {
  return x;
}

interface Stepper {
  step(delta: 1 | 2): 1 | 2;
}

class Counter implements Stepper {
  step(delta: 1 | 2): 1 | 2 {
    return delta === 1 ? 2 : 1;
  }

  static genericStep<T>(_tag: T, delta: 1 | 2): 1 | 2 {
    return delta;
  }
}

class TaggedCounter<T> {
  step(_tag: T, delta: 1 | 2): 1 | 2 {
    return delta;
  }
}

function main(): 0 | 1 | 2 {
  const c: Code = pickCode(false);
  assert(JSON.stringify(c) === "2", "stringify aliased number literal union");

  const f: 1.5 | 2.5 = pickFloat(false);
  assert(JSON.stringify(f) === "2.5", "stringify float literal union");

  const nested: NestedCode = pickNested(3);
  assert(JSON.stringify(nested) === "3", "stringify nested number literal union");

  const b: true | false = pickBool(true);
  assert(JSON.stringify(b) === "true", "stringify boolean literal union");

  // Parameters keep the union type; a `const` would narrow to its literal.
  assert(
    indented({ code: c }, 4) === "{\n    \"code\": 2\n}",
    "stringify with number literal union space",
  );
  assert(
    padded({ code: c }, "\t") === "{\n\t\"code\": 2\n}",
    "stringify with string literal union space",
  );
  assert(
    templated([1], "--") === "[\n--1\n]",
    "stringify with template literal space",
  );

  // Generic functions returning number-literal unions (direct, boxed, widened).
  assert(String(genericTwo(0)) === "2", "String(generic 1 | 2)");
  const n: number = genericTwo("hello");
  assert(n === 2, "assign generic 1 | 2 to number");
  const widened: unknown = genericTwo(true);
  assert(widened === 2, "widen generic 1 | 2 to unknown");
  assert(genericQuad("x", true) === 3, "generic 0 | 1 | 2 | 3");
  assert(genericSign(false) === -1, "generic -1 | 1 negative");
  assert(genericSign(true) === 1, "generic -1 | 1 positive");

  // Erased generic union instantiated to a number-literal union (`T | 1` with `T = 2`).
  const erasedToNumUnion: 1 | 2 = genericOrOne<2>(2, true);
  assert(erasedToNumUnion === 2, "erased T | 1 instantiated with 2");
  assert(genericOrOne<2>(2, false) === 1, "erased T | 1 fallback 1");

  // Sibling union shapes from generic functions.
  assert(genericOptional(1, true) === 2, "generic 1 | 2 | undefined present");
  assert(genericOptional(1, false) === undefined, "generic 1 | 2 | undefined absent");
  assert(genericMixed(1, true) === 1, "generic 1 | 'a' number");
  assert(genericMixed(1, false) === "a", "generic 1 | 'a' string");
  assert(genericStrings(true) === "a", "generic 'a' | 'b'");
  assert(genericBool("yes") === true, "generic true | false");
  assert(id<1 | 2>(2) === 2, "id<1 | 2>(2)");

  // Class and interface methods with number-literal union parameters and returns.
  const counter = new Counter();
  const stepper: Stepper = counter;
  assert(counter.step(1) === 2, "class method 1 | 2");
  assert(stepper.step(2) === 1, "interface method 1 | 2");
  assert(Counter.genericStep("ok", 2) === 2, "static generic method 1 | 2");
  assert(new TaggedCounter<string>().step("ok", 1) === 1, "generic class method 1 | 2");

  return c;
}
