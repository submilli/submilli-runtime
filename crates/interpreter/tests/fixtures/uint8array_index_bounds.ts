// A `Uint8Array` index out of range raises the same catchable `RangeError` an
// array index does, on both the read and the write path.
function main(): void {
  const u = Uint8Array.alloc(2);

  let readMessage = "";
  try {
    const x = u[5];
    assert(false, "an out-of-range read should not return");
  } catch (e) {
    readMessage = e.message;
  }
  assert(readMessage === "index out of range", "the read throws RangeError");

  let writeMessage = "";
  try {
    u[9] = 3;
    assert(false, "an out-of-range write should not return");
  } catch (e) {
    writeMessage = e.message;
  }
  assert(writeMessage === "index out of range", "the write throws RangeError");

  try {
    const y = u[5];
    assert(false, "unreachable");
  } catch (e: RangeError) {
    assert(e.name === "RangeError", "catchable by the RangeError class");
  }

  const arr: number[] = [1, 2];
  let arrayMessage = "";
  try {
    const z = arr[5];
    assert(false, "unreachable");
  } catch (e) {
    arrayMessage = e.message;
  }
  assert(arrayMessage === readMessage, "same message as the array path");

  u[0] = 7;
  u[1] = 255;
  assert(u[0] === 7, "in-range write then read");
  assert(u[1] === 255, "the last in-range slot is reachable");
  assert(u[u[0] - 7] === 7, "a nested index expression");

  // A zero-length array has no reachable slot at all.
  const empty = Uint8Array.alloc(0);
  let emptyRead = "";
  try {
    const z = empty[0];
    assert(false, "unreachable");
  } catch (e) {
    emptyRead = e.message;
  }
  assert(emptyRead === "index out of range", "slot 0 of an empty array is out of range");

  // Left-to-right evaluation survives the check: the index expression and the
  // RHS both run, in that order, before the bounds check throws.
  order = "";
  try {
    u[outOfRangeIndex()] = sideEffectingValue();
    assert(false, "unreachable");
  } catch (e) {
    order = order + "!";
  }
  assert(order === "iv!", "index, then RHS, then the throw");
}

let order: string = "";

function outOfRangeIndex(): number {
  order = order + "i";
  return 99;
}

function sideEffectingValue(): number {
  order = order + "v";
  return 3;
}
