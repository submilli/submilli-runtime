// Host- and codegen-thrown TypeErrors: a runtime-checked cast mismatch
// (including JSON.parse<T> shapes) and a fatal TextDecoder.decode both land
// in a `catch (e: TypeError)` arm.
function main(): void {
  // JSON.parse returns unknown; the `as` validator throws TypeError on mismatch.
  let cast = "";
  try {
    const n = JSON.parse("\"hello\"") as number;
    assert(false, "cast mismatch should have thrown");
  } catch (e: TypeError) {
    cast = e.name + ":" + e.message;
  }
  assert(
    cast === "TypeError:type mismatch: expected number, got string",
    "cast mismatch is TypeError",
  );

  // A mismatched cast still lands in a broader `catch (e: Error)` arm.
  let viaBase = "";
  try {
    const o = JSON.parse("5") as { name: string };
    assert(false, "shape mismatch should have thrown");
  } catch (e: Error) {
    viaBase = e.name;
  }
  assert(viaBase === "TypeError", "TypeError caught as Error keeps its name");

  // Invalid UTF-8 input makes TextDecoder.decode throw a TypeError.
  const bad = new Uint8Array([255]);
  let decoded = "";
  try {
    decoded = new TextDecoder().decode(bad);
    assert(false, "invalid UTF-8 should have thrown");
  } catch (e: TypeError) {
    decoded = e.name;
  }
  assert(decoded === "TypeError", "fatal decode is TypeError");

  // A RangeError does not enter the TypeError arm.
  let arm = "";
  try {
    "x".repeat(-1);
  } catch (e: TypeError) {
    arm = "type";
  } catch (e: RangeError) {
    arm = "range";
  }
  assert(arm === "range", "RangeError skips the TypeError arm");
}
