// A user generic parameter has no runtime representation, so `get<T>` inside a
// generic function has nothing to test the stored value against. The same gate
// that screens `as` targets rejects it, naming erasure as the reason.
// expect-error: generic type parameters are erased at runtime
import session from "submilli:session";

function load<T>(key: string): T {
  return session.get<T>(key);
}

function main(): void {
  session.set("k", 1);
  const n = load<number>("k");
  assert(n === 1, "unreachable");
}
