// An array index step inside a chain is bounds-checked like one outside it: a
// catchable `MissingValueError`, not a Wasm trap the program cannot see. Without the
// check the chain's `array.get` runs unguarded and takes the whole process down,
// so the `catch` below never runs.
//
// This fixture exercises array bounds; byte and tuple chain reads share the
// corresponding non-chain index paths.
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
    plain = e instanceof MissingValueError ? "MissingValueError" : "other";
  }
  assert(plain === "MissingValueError", `non-chain index, got ${plain}`);

  let chained = "none";
  try {
    console.log(`${maybe?.arr[99].toFixed(0)}`);
  } catch (e) {
    chained = e instanceof MissingValueError ? "MissingValueError" : "other";
  }
  assert(chained === "MissingValueError", `chain index, got ${chained}`);

  // As the chain's last step, a missing element is the chain's `undefined`.
  assert((maybe?.arr[99] ?? -1) === -1, "a missing last step reads as undefined");

  const none: Bag | null = null as Bag | null;
  assert(none?.arr[99] === undefined, "a short-circuit never reaches the index");
}
