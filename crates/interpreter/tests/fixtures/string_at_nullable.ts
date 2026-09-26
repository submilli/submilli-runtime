// `String#at` answers `null` out of range, matching `Array#at` and
// `Uint8Array#at`. `charAt` keeps the `""`-on-miss form.
function main(): void {
  const s = "abc";

  assert(s.at(99) === null, "past the end is null");
  assert(s.at(-99) === null, "before the start is null");
  assert(s.at(3) === null, "index === length is null");

  const hit = s.at(0);
  assert(hit !== null, "an in-range read is non-null");
  if (hit !== null) {
    assert(hit === "a", "narrows to the character");
    assert(hit.length === 1, "and is usable as a string once narrowed");
  }

  assert(s.at(-1) === "c", "a negative index counts from the end");
  assert(s.at(-3) === "a", "at(-length)");

  assert(s.charAt(99) === "", "charAt still answers the empty string");
  assert("".at(0) === null, "the empty string has no character 0");

  // The three indexable types now agree on the out-of-range answer.
  const arr: string[] = ["a"];
  assert(arr.at(9) === null, "Array#at");
  assert(Uint8Array.fromArray([1]).at(9) === null, "Uint8Array#at");

  // The union flows through the places a `string | null` normally goes.
  assert(s.at(0)?.length === 1, "optional chain on a hit");
  assert(s.at(9)?.length === null, "optional chain short-circuits on a miss");
  assert(s.at(0) !== null && s.at(0)!.length === 1, "`!` narrows the miss away");

  const receiver: string | null = "abc";
  assert(receiver?.at(0) === "a", "a nullable receiver");
  const gone: string | null = null as string | null;
  assert(gone?.at(0) === null, "a null receiver short-circuits");

  assert(JSON.stringify({ hit: s.at(0), miss: s.at(9) }) === "{\"hit\":\"a\",\"miss\":null}",
    "the union serializes");

  const nan = 0 / 0;
  assert(s.at(nan) === "a", "a NaN index truncates to 0, like charAt");
  assert(s.at(1 / 0) === null, "+Infinity is out of range");
  assert(s.at(-1 / 0) === null, "-Infinity is out of range");
}
