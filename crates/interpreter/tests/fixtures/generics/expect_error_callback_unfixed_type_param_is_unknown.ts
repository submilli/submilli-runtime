// The result inferred from a callback that reads a still-unfixed type
// parameter is `unknown`, as tsc infers it.
// expect-error: expected `number`, got `unknown`
// expect-error-count: 1
function memo<K, V>(o: { compute: (cache: Map<K, V>) => V }, key: K): V {
  return o.compute(new Map<K, V>());
}

function main(): void {
  const v = memo({ compute: (cache) => cache.get("abc") }, "abc");
  const n: number = v;
}
