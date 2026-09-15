// `new Uint8Array(n)` — the JS length form — alongside the array form. Both
// arms share one union-typed slot; the host picks by runtime shape.
function main(): void {
  const a = new Uint8Array(3);
  assert(a.length === 3, "length form reserves n bytes");
  assert(a[0] === 0 && a[1] === 0 && a[2] === 0, "zero-filled");

  const fromArray = new Uint8Array([1, 2, 300]);
  assert(fromArray.length === 3, "array form keeps one byte per element");
  assert(fromArray[2] === 44, "array elements truncate to the low 8 bits");

  assert(new Uint8Array(0).length === 0, "zero length");
  assert(new Uint8Array([]).length === 0, "empty array");

  const n = 4;
  assert(new Uint8Array(n).length === 4, "length from a variable, not a literal");
  assert(new Uint8Array(2.7).length === 2, "a fractional length truncates");

  const viaAlloc = Uint8Array.alloc(3);
  assert(viaAlloc.length === a.length, "the length form agrees with alloc");

  // The arm is picked at runtime, so a union-typed variable reaches both.
  const values: number[] = [1, 2, 3];
  assert(new Uint8Array(values).length === 3, "a number[] variable, not a literal");
  const pair: [number, number] = [4, 5];
  assert(new Uint8Array(pair)[1] === 5, "a tuple argument");

  let either: number[] | number = 3;
  assert(new Uint8Array(either).length === 3, "union-typed variable, number arm");
  either = [1, 2];
  assert(new Uint8Array(either).length === 2, "union-typed variable, array arm");

  assert(new Uint8Array(0 / 0).length === 0, "a NaN length is 0");

  // A length past the ceiling is refused by name rather than attempting the
  // allocation — `f64 as u32` saturates upward, so this would ask for 4 GiB.
  let refused = "";
  try {
    const huge = new Uint8Array(1 / 0);
    assert(false, "an infinite length should not allocate");
  } catch (e: RangeError) {
    refused = e.message;
  }
  assert(refused.includes("invalid Uint8Array length"), "the ceiling is named");
  assert(refused.includes("maximum"), "and so is the maximum");

  let allocRefused = "";
  try {
    const huge = Uint8Array.alloc(4294967296);
    assert(false, "alloc refuses the same way");
  } catch (e: RangeError) {
    allocRefused = e.message;
  }
  assert(allocRefused.includes("invalid Uint8Array length"), "alloc shares the ceiling");

  // A negative length is refused too, rather than silently giving an empty
  // buffer where the program asked for a sized one. JS raises here as well.
  let negative = "";
  try {
    const bad = new Uint8Array(-1);
    assert(false, "a negative length should not allocate");
  } catch (e: RangeError) {
    negative = e.message;
  }
  assert(negative.includes("cannot be negative"), "a negative length is refused by name");
  // Truncation happens first, the way JS's `ToIndex` does, so a fraction that
  // truncates to zero is a legal empty length rather than a negative one.
  assert(new Uint8Array(-0.5).length === 0, "-0.5 truncates to an empty array");
  assert(Uint8Array.alloc(-0.9999).length === 0, "so does -0.9999");

  let negativeAlloc = "";
  try {
    const bad = Uint8Array.alloc(-1 / 0);
    assert(false, "-Infinity is not a length");
  } catch (e: RangeError) {
    negativeAlloc = e.message;
  }
  assert(negativeAlloc.includes("cannot be negative"), "-Infinity is refused");
}
