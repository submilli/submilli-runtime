// `Map` keys, `Set` elements and `Array#includes` compare by SameValueZero, as
// in JavaScript: `NaN` matches `NaN`, and `-0` matches `+0` and is stored as
// `+0`. `indexOf` keeps `===`, so it never finds `NaN`.
function main(): void {
  const nan = 0 / 0;
  const m = new Map<number, string>();
  m.set(nan, "first");
  m.set(nan, "second");
  assert(m.size === 1 && m.get(nan) === "second" && m.has(nan), "NaN is one Map key");
  assert(m.delete(nan) && m.size === 0, "a NaN key can be deleted");

  const s = new Set<number>([nan, nan, 1]);
  assert(s.size === 2 && s.has(nan), "NaN is one Set element");

  const z = new Map<number, string>();
  z.set(-0, "zero");
  assert(z.get(0) === "zero" && z.size === 1, "-0 and +0 are one key");
  for (const k of z.keys()) {
    assert(1 / k === Infinity, "a -0 key is stored as +0");
  }
  const zs = new Set<number>([-0]);
  for (const v of zs) {
    assert(1 / v === Infinity, "a -0 element is stored as +0");
  }

  const arr: number[] = [1, nan, 3];
  assert(arr.includes(nan), "includes finds NaN");
  assert(arr.indexOf(nan) === -1 && arr.lastIndexOf(nan) === -1, "indexOf never finds NaN");
  assert([-0].includes(0) && [0].indexOf(-0) === 0, "zeros match either way");
}
