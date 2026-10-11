// `String#at` answers `undefined` out of range, matching `Array#at` and
// `Uint8Array#at`. `charAt` keeps the `""`-on-miss form.
function main(): void {
  const s = "abc";

  assert(s.at(99) === undefined, "past the end is undefined");
  assert(s.at(-99) === undefined, "before the start is undefined");
  assert(s.at(3) === undefined, "index === length is undefined");

  const hit = s.at(0);
  assert(hit !== undefined, "an in-range read is non-undefined");
  if (hit !== undefined) {
    assert(hit === "a", "narrows to the character");
    assert(hit.length === 1, "and is usable as a string once narrowed");
  }

  assert(s.at(-1) === "c", "a negative index counts from the end");
  assert(s.at(-3) === "a", "at(-length)");

  assert(s.charAt(99) === "", "charAt still answers the empty string");
  assert("".at(0) === undefined, "the empty string has no character 0");

  // The three indexable types now agree on the out-of-range answer.
  const arr: string[] = ["a"];
  assert(arr.at(9) === undefined, "Array#at");
  assert(Uint8Array.fromArray([1]).at(9) === undefined, "Uint8Array#at");

  // The union flows through the places a `string | undefined` normally goes.
  assert(s.at(0)?.length === 1, "optional chain on a hit");
  assert(s.at(9)?.length === undefined, "optional chain short-circuits on a miss");
  assert(s.at(0) !== undefined && s.at(0)!.length === 1, "`!` narrows the miss away");

  const receiver: string | null = "abc";
  assert(receiver?.at(0) === "a", "a nullable receiver");
  const gone: string | null = null as string | null;
  assert(gone?.at(0) === undefined, "a null receiver short-circuits");

  assert(JSON.stringify({ hit: s.at(0), miss: s.at(9) }) === "{\"hit\":\"a\"}",
    "the union serializes");

  const nan = 0 / 0;
  assert(s.at(nan) === "a", "a NaN index truncates to 0, like charAt");
  assert(s.at(1 / 0) === undefined, "+Infinity is out of range");
  assert(s.at(-1 / 0) === undefined, "-Infinity is out of range");
}
