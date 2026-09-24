function asserted<T>(value: T): T { return value!; }
function coalesced<T>(value: T): T { return value ?? value; }
function narrowedWrite<T>(value: T): T | null {
  let result: T | null = null;
  if (typeof value === "string" || typeof value === "number") {
    result = value;
  }
  return result;
}
function main(): void {
  assert(asserted("s") === "s");
  assert(coalesced("s") === "s");
  assert(narrowedWrite("a") === "a");
  assert(narrowedWrite(3) === 3);
  assert(narrowedWrite(true) === null);
  const value: unknown = "erased";
  assert(asserted<unknown>(value) === "erased");
}
