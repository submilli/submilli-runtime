// The host-built `$boxed_number` / `$boxed_boolean` vtables (SUB-590): numbers
// and booleans flowing through `(ref $Object)` slots must dispatch toString /
// toJson / equals / hash to the Rust slots, identically to the old Wasm bodies.

function main(): void {
  // toString slot — `Array#join` re-enters each element's toString slot.
  assert([1, 2, 3].join("-") === "1-2-3", "boxed number toString via join");
  assert([true, false, true].join(",") === "true,false,true", "boxed boolean toString via join");

  // toJson slot — JSON.stringify of arrays/objects re-enters each value's toJson.
  assert(JSON.stringify([1.5, 2, 3.25]) === "[1.5,2,3.25]", "boxed number toJson");
  assert(JSON.stringify([true, false]) === "[true,false]", "boxed boolean toJson");
  assert(JSON.stringify({ n: 7, b: true }) === "{\"b\":true,\"n\":7}", "boxed values in object toJson");

  // equals + hash slots — Set/Map keying on boxed primitives.
  const ns = new Set<number>();
  ns.add(10);
  ns.add(10);
  ns.add(20);
  assert(ns.size === 2, "boxed number equals/hash dedupes");
  assert(ns.has(20) && !ns.has(30), "boxed number membership");

  const bs = new Set<boolean>();
  bs.add(true);
  bs.add(true);
  bs.add(false);
  assert(bs.size === 2, "boxed boolean equals/hash dedupes");
  assert(bs.has(false), "boxed boolean membership");

  const m = new Map<number, string>();
  m.set(1, "one");
  m.set(1, "uno");
  m.set(2, "two");
  assert(m.size === 2, "number-keyed map overwrites equal keys");
  assert(m.get(1) === "uno" && m.get(2) === "two", "number-keyed map lookup");
}
