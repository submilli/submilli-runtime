function main(): void {
  // Constructible, with the right name and message.
  const e = new RangeError("out of range");
  assert(e.message === "out of range", "message field");
  assert(e.name === "RangeError", "name field");
  assert(e instanceof RangeError, "instanceof own class");
  assert(e instanceof Error, "instanceof parent");
  assert(Error.isError(e), "Error.isError sees the subclass");
  assert(e.toString() === "RangeError: out of range", "toString");

  // A base Error is not a RangeError.
  const base = new Error("plain");
  assert(!(base instanceof RangeError), "base Error is not RangeError");

  // Typed catch filters: RangeError arm binds a thrown RangeError.
  let caught = "";
  try {
    throw new RangeError("thrown");
  } catch (e: RangeError) {
    caught = e.name + ":" + e.message;
  }
  assert(caught === "RangeError:thrown", "typed catch binds RangeError");

  // A base Error skips the RangeError arm and lands in the catch-all.
  let arm = "";
  try {
    throw new Error("base");
  } catch (e: RangeError) {
    arm = "range";
  } catch (e) {
    arm = "base:" + e.name;
  }
  assert(arm === "base:Error", "base Error skips the RangeError arm");

  // An out-of-range write throws a RangeError. A read finds no value and
  // throws `MissingValueError`, which is not one.
  const nums: number[] = [1, 2, 3];
  let indexed = "";
  try {
    nums[7] = 4;
  } catch (e: RangeError) {
    indexed = e.name + ":" + e.message;
  }
  assert(indexed === "RangeError:index 7 is out of range for `nums` (length 3); use `nums.push(value)` to append instead", "arr[i] = v OOB is RangeError");
  let read = "";
  try {
    const x = nums[7];
    read = x.toString();
  } catch (e: RangeError) {
    read = "range";
  } catch (e) {
    read = e.name;
  }
  assert(read === "MissingValueError", "arr[i] OOB read is not a RangeError");
}
