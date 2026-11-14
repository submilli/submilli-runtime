// A class instance method with its own type parameters: called with explicit
// and inferred type arguments, overridden, read through an interface, and
// checked at run time through its own `T`.
import session from "submilli:session";

interface Wrapper {
  wrap<T>(value: T): T[];
}

class Util implements Wrapper {
  label: string = "util";
  identity<U>(v: U): U {
    return v;
  }
  wrap<T>(value: T): T[] {
    return [value];
  }
  load<T>(key: string): T | undefined {
    return session.get<T>(key);
  }
}

class Twice extends Util {
  wrap<T>(value: T): T[] {
    return [value, value];
  }
}

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

function through(wrapper: Wrapper): number {
  return wrapper.wrap<string>("x").length;
}

function main(): void {
  const util = new Util();
  assert(util.identity<number>(1) === 1, "explicit type argument");
  assert(util.identity("s") === "s", "inferred type argument");
  const twice: Util = new Twice();
  assert(twice.wrap(1).length === 2, "an override is dispatched");
  assert(through(util) === 1 && through(new Twice()) === 2, "through an interface");

  session.set("k", 1);
  assert(util.load<number>("k") === 1, "its own `T` reaches the host check");
  assert(rejects(() => { util.load<string>("k"); }), "and is checked");
}
