// An object literal passed for a type parameter keeps the boolean literals
// the bound parameter admits as its own type, as in TypeScript, while the
// type argument stays `boolean`, so the result is still writable.
function id<V>(value: V): V {
  return value;
}

function main(): void {
  const flags = id({ on: true, count: 1 });
  flags.on = false;
  flags.count += 1;
  assert(!flags.on && flags.count === 2, "the bound type is widened");

  const nested = id({ inner: { on: true } });
  nested.inner.on = false;
  assert(!nested.inner.on, "nested literals too");
}
