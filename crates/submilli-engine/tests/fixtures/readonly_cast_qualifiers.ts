type Numbers = readonly number[];
function main(): void {
  const ro: readonly number[] = [1];
  const direct = ro as readonly number[];
  const nullable = ro as readonly number[] | null;
  const alias = ro as Numbers | null;
  const nested: { items: readonly number[] } = { items: ro };
  const nestedCast = nested as { items: readonly number[] } | null;
  const erased: unknown = nested;
  const checked = erased as { items: readonly number[] };
  assert(checked.items[0] === 1, "checked nested cast");
  const tuple: readonly [number, string] = [2, "ok"];
  const tupleCast = tuple as readonly [number, string] | null;
  assert(direct[0] === 1, "direct");
  assert(nullable !== null && nullable[0] === 1, "nullable");
  assert(alias !== null && alias[0] === 1, "alias");
  assert(nestedCast !== null && nestedCast.items[0] === 1, "nested");
  assert(tupleCast !== null && tupleCast[1] === "ok", "tuple");
}
