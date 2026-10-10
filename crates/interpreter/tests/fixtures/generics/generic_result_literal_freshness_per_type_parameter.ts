// Whether a fresh literal in a generic call's result widens at a `let` is
// decided per type parameter, as in tsc: a declared `"on"` bound to `V`
// reaches `K | D | V[]` only inside `V[]`, so the fresh `"off"` given for `D`
// still widens. A result with several bare type parameters takes the whole
// expected type for each, so an object literal argument for one of them is
// checked against all of it.
type Mode = "on" | "off";

interface Box<T> {
  v: T;
}

function keyOr<K, D, V>(entry: [K, V], fallback: D): K | D | V[] {
  return fallback;
}

function orElse<A, B>(a: A, b: Box<B>): A | B {
  return a;
}

function main(): void {
  let m: Mode = "on";
  if (m.length > 5) {
    m = "off";
  }
  const entry: [number, Mode] = [1, "on"];
  let found = keyOr(entry, "off");
  found = "missing";
  const r: Mode = orElse(m, { v: m });
  assert(found === "missing" && r === "on", "per type parameter");
}
