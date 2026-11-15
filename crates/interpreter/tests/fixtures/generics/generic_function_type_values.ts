// Values of a generic function type: assigned to one, instantiated to a
// non-generic type, and checked at run time after instantiation.
import session from "submilli:session";

type Identity = <T>(value: T) => T;

const loader = {
  load<T>(key: string): T | undefined {
    return session.get<T>(key);
  },
};

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

function main(): void {
  const id: Identity = <T,>(value: T): T => value;
  assert(id<number>(1) === 1, "explicit type argument through an alias");
  assert(id("s") === "s", "inferred through an alias");

  const numberId: (value: number) => number = id;
  assert(numberId(2) === 2, "instantiated to a non-generic type");

  session.set("k", 1);
  const loadNumber: (key: string) => number | undefined = loader.load;
  assert(loadNumber("k") === 1, "an instantiation binds its descriptors");
  const loadString: (key: string) => string | undefined = loader.load;
  assert(rejects(() => { loadString("k"); }), "an instantiation is checked");
}
