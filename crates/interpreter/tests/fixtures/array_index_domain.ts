// An index is in range only when it is a non-negative integer below the length.
// `i32.trunc_sat_f64_u` saturates a negative, `NaN`, or fractional index to `0`,
// so without an `f64`-level check each of these silently aliased slot 0 — a read
// returning the wrong element and a write corrupting it.
function message(f: () => void): string {
  try {
    f();
    return "no throw";
  } catch (e: RangeError) {
    return e.message;
  }
}

function main(): void {
  const a: number[] = [1, 2];

  assert(message(() => { const x = a[-1]; }) === "index out of range", "negative read");
  assert(message(() => { a[-1] = 99; }) === "index out of range", "negative write");
  assert(message(() => { const x = a[0 / 0]; }) === "index out of range", "NaN read");
  assert(message(() => { a[0 / 0] = 99; }) === "index out of range", "NaN write");
  assert(message(() => { const x = a[0.5]; }) === "index out of range", "fractional read");
  assert(message(() => { a[1.5] = 99; }) === "index out of range", "fractional write");
  assert(message(() => { const x = a[-1 / 0]; }) === "index out of range", "-Infinity read");
  assert(message(() => { const x = a[1 / 0]; }) === "index out of range", "Infinity read");
  assert(message(() => { const x = a[1e300]; }) === "index out of range", "huge finite read");
  assert(message(() => { a[-1] = a[-1] + 1; }) === "index out of range", "compound write");

  // Nothing above reached storage.
  assert(a[0] === 1, "slot 0 untouched by the rejected writes");
  assert(a[1] === 2, "slot 1 untouched by the rejected writes");

  // `-0` is a non-negative integer and indexes slot 0, as in JS.
  assert(a[-0] === 1, "-0 reads slot 0");
  a[-0] = 7;
  assert(a[0] === 7, "-0 writes slot 0");

  // The same check backs `Uint8Array`.
  const u = Uint8Array.alloc(2);
  assert(message(() => { const x = u[-1]; }) === "index out of range", "Uint8Array negative read");
  assert(message(() => { u[-1] = 5; }) === "index out of range", "Uint8Array negative write");
  assert(message(() => { const x = u[0.5]; }) === "index out of range", "Uint8Array fractional read");
  assert(message(() => { const x = u[0 / 0]; }) === "index out of range", "Uint8Array NaN read");
  u[1] = 9;
  assert(u[0] === 0, "Uint8Array slot 0 untouched");
  assert(u[1] === 9, "an in-range Uint8Array write still lands");

  // An index computed at runtime is checked the same way.
  let i = 2;
  i = i - 3;
  assert(message(() => { const x = a[i]; }) === "index out of range", "computed negative index");

  // The two ends of the in-range interval.
  assert(a[0] === 7 && a[1] === 2, "both slots are readable");
  assert(message(() => { const x = a[2]; }) === "index out of range", "exactly at the length");
  const empty: number[] = [];
  assert(message(() => { const x = empty[0]; }) === "index out of range", "empty array");
  assert(message(() => { const x = empty[-0]; }) === "index out of range", "-0 on an empty array");

  // Large magnitudes and values that are integers only after rounding.
  assert(message(() => { const x = a[2147483648]; }) === "index out of range", "2**31");
  assert(message(() => { const x = a[4294967295]; }) === "index out of range", "2**32 - 1");
  assert(message(() => { const x = a[9007199254740992]; }) === "index out of range", "2**53");
  assert(message(() => { const x = a[-1e-320]; }) === "index out of range", "a denormal below zero");
  assert(message(() => { const x = a[0.9999999999999999]; }) === "index out of range", "just under 1");

  // A write's RHS runs before the range check throws (left-to-right order).
  order = "";
  assert(message(() => { a[-1] = tick(9); }) === "index out of range", "out-of-range write");
  assert(order === "R", "the RHS ran before the check threw");

  // Every receiver shape routes through the same check.
  const h = new Holder();
  assert(message(() => { const x = h.arr[-1]; }) === "index out of range", "field receiver, read");
  assert(message(() => { h.arr[-1] = 99; }) === "index out of range", "field receiver, write");
  assert(h.arr[0] === 1, "the field's slot 0 is untouched");
  assert(message(() => { const x = h.get()[0.5]; }) === "index out of range", "method-result receiver");
  assert(message(() => { const g = (): number => a[-1]; const y = g(); }) === "index out of range", "inside a closure");

  // `.at(i)` is the documented non-throwing alternative and keeps JS clamping.
  assert(a.at(-1) === 2, "`.at` counts from the end");
  assert(a.at(0.5) === 7, "`.at` truncates a fractional index");
  assert(a.at(9) === null, "`.at` returns null past the end");
}

let order: string = "";

function tick(n: number): number {
  order = order + "R";
  return n;
}

class Holder {
  arr: number[] = [1, 2, 3];
  get(): number[] {
    return this.arr;
  }
}
