// A callback whose parameter type mentions a type parameter nothing has
// fixed yet sees that parameter as `unknown`, as in tsc. Its result, which
// mentions the parameter again (`Map<K, V>.get` returns `V | undefined`),
// infers `unknown` instead of a `V` that contains itself.
function memo<K, V>(o: { compute: (cache: Map<K, V>) => V }, key: K): V {
  return o.compute(new Map<K, V>());
}

function memoCurried<K, V>(o: { compute: (cache: Map<K, V>) => (k: K) => V }, key: K): V {
  return o.compute(new Map<K, V>())(key);
}

function main(): void {
  const v = memo({ compute: (cache) => cache.get("abc") }, "abc");
  assert(v === undefined, "an empty cache has no entry");
  const u = memoCurried({
    compute: (cache) => (k) => {
      const hit = cache.get(k);
      return hit;
    },
  }, "abc");
  assert(u === undefined, "the curried form infers the same way");
  const narrowed: unknown = v;
  assert(typeof narrowed === "undefined", "the result is unknown and holds undefined");
}
