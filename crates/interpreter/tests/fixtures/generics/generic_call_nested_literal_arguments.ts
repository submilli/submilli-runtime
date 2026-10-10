// A generic call whose literal arguments bind its type parameter to their
// union may itself be the argument of another generic call, as in tsc: the
// outer call's unbound type parameter says nothing about the inner one.
function first<T>(a: T, b: T): T {
  return a;
}

function id<T>(x: T): T {
  return x;
}

function wrap<T>(x: T): T[] {
  return [x];
}

function orNull<T>(x: T | null): T[] {
  return x === null ? [] : [x];
}

function pair<K>(entry: [K, number]): K {
  return entry[0];
}

function main(): void {
  const n = id(first(1, 2));
  const words = wrap(first("a", "b"));
  const key = pair([first("a", "b"), 2]);
  const sizes = new Map([[first("x", "y"), 1]]);
  const kept = orNull(first("a", "b"));
  const single = orNull(id(1));
  assert(n === 1 && words[0] === "a" && key === "a" && sizes.get("x") === 1, "nested literals");
  assert(kept[0] === "a" && single[0] === 1, "an outer T | null");
}
