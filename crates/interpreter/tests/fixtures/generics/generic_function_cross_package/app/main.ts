import { identity, load, store, Store, Wrapper, Identity, Logger, logWith } from "@test/tools";
import session from "submilli:session";

class Quiet extends Logger {
  log(value: unknown): number {
    return 2;
  }
}

class Twice extends Wrapper {
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

function copy(source: Store): Store {
  return { ...source };
}

function main(): void {
  assert(identity<number>(1) === 1, "an imported generic function value");
  const id: Identity = identity;
  assert(id("s") === "s", "assigned to the imported alias");
  const read = load;
  session.set("k", 1);
  assert(read<number>("k") === 1, "an imported generic function read as a value");
  assert(rejects(() => { read<string>("k"); }), "keeps its check");
  assert(store.read<number>("k") === 1, "an imported interface's generic method");
  assert(rejects(() => { copy(store).read<string>("k"); }), "a spread copy keeps the check");
  const loadNumber: (key: string) => number | undefined = load;
  assert(loadNumber("k") === 1, "an instantiated imported generic function");
  const loadString: (key: string) => string | undefined = load;
  assert(rejects(() => { loadString("k"); }), "an instantiation keeps its check");
  const wrapper: Wrapper = new Twice();
  assert(wrapper.wrap(1).length === 2, "a local override of an imported generic method");
  assert(new Wrapper().wrap<string>("x").length === 1, "an imported generic class method");
  assert(new Quiet().log("a") === 2, "an override without type parameters");
  assert(logWith(new Quiet()) === 2, "called from the package through the base class");
}
