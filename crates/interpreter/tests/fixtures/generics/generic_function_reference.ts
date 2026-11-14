// A generic top-level function read as a value has a generic function type:
// called with type arguments, instantiated, and checked at run time through
// its own `T`.
import session from "submilli:session";

function identity<T>(value: T): T {
  return value;
}

function load<T>(key: string): T | undefined {
  return session.get<T>(key);
}

function pair<T>(first: T, second?: T): T[] {
  return second === undefined ? [first] : [first, second];
}

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

function shadow<T>(value: T): T {
  // `identity`'s own `T` is not this function's `T`.
  const id = identity;
  return id<T>(value);
}

function main(): void {
  const id = identity;
  assert(id<number>(1) === 1, "a generic function value takes type arguments");
  assert(id("s") === "s", "and infers them");
  assert(id === identity, "a function read twice is the same value");

  const numberId: (value: number) => number = identity;
  assert(numberId(2) === 2, "instantiated to a non-generic type");

  session.set("k", 1);
  const read = load;
  assert(read<number>("k") === 1, "its own `T` reaches the host check");
  assert(rejects(() => { read<string>("k"); }), "and is checked");
  const readString: (key: string) => string | undefined = load;
  assert(rejects(() => { readString("k"); }), "an instantiation is checked");

  const p = pair;
  assert(p(1).length === 1 && p(1, 2).length === 2, "optional parameters pass through");
  assert(shadow(3) === 3, "a shadowed type parameter");
  statics();
}

class Tools {
  static wrap<T>(value: T): T[] {
    return [value];
  }
}

function statics(): void {
  const wrap = Tools.wrap;
  assert(wrap<number>(1)[0] === 1, "a generic static read as a value");
}
