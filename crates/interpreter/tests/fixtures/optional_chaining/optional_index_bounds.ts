// An array index step inside a chain is bounds-checked like one outside it: a
// catchable `RangeError`, not a Wasm trap the program cannot see. Without the
// check the chain's `array.get` runs unguarded and takes the whole process down,
// so the `catch` below never runs.
//
// Arrays only — a chain index step rejects `Uint8Array`, tuple, and string
// receivers that the non-chain form accepts (SUB-778).
class Bag {
  arr: number[] = [1, 2, 3];
}

function main(): void {
  const bag = new Bag();
  const maybe: Bag | null = bag;

  assert(maybe?.arr[0] === 1, "in-bounds read through the chain");

  let plain = "none";
  try {
    console.log(`${bag.arr[99]}`);
  } catch (e) {
    plain = e instanceof RangeError ? "RangeError" : "other";
  }
  assert(plain === "RangeError", `non-chain index, got ${plain}`);

  let chained = "none";
  try {
    console.log(`${maybe?.arr[99] ?? -1}`);
  } catch (e) {
    chained = e instanceof RangeError ? "RangeError" : "other";
  }
  assert(chained === "RangeError", `chain index, got ${chained}`);

  const none: Bag | null = null;
  assert(none?.arr[99] === null, "a short-circuit never reaches the index");
}
