function main(): void {
  const reviews = new Map<string, string>();
  reviews.set("r1", "Arrived late");
  reviews.set("r2", "Works great");
  const result = Object.fromEntries(reviews);
  const text: string | undefined = result.r1;
  assert(text === "Arrived late");
  assert(result.r2 === "Works great");
  assert(result.missing === undefined);
  assert(JSON.stringify({ reviews: result }) === '{"reviews":{"r1":"Arrived late","r2":"Works great"}}');
  result.r3 = "New review";
  assert(result.r3 === "New review");
  assert(reviews.size === 2);

  const pairs: [string, number][] = [["b", 1], ["a", 2], ["b", 3], ["__proto__", 4]];
  const numbers: { [key: string]: number } = Object.fromEntries(pairs);
  assert(numbers.b === 3);
  assert(numbers.a === 2);
  assert(numbers["__proto__"] === 4);
  assert(Object.hasOwn(numbers, "__proto__"));
  assert(Object.keys(numbers).join(",") === "b,a,__proto__");
  assert(Object.fromEntries(reviews.entries()).r1 === "Arrived late");
  assert(Object.keys(Object.fromEntries(new Map<string, number>())).length === 0);

  const numeric: [number, string][] = [[-0, "zero"], [12, "twelve"], [NaN, "nan"]];
  const keys = Object.fromEntries(numeric);
  assert(keys["0"] === "zero");
  assert(keys["12"] === "twelve");
  assert(keys["NaN"] === "nan");

  const lone = String.fromCharCode(55296);
  const utf16: [string, string][] = [[lone, lone], ["\ufffd", "replacement"]];
  const preserved = Object.fromEntries(utf16);
  assert(preserved[lone] === lone);
  assert(preserved["\ufffd"] === "replacement");
  assert(Object.keys(preserved).length === 2);

  const values: [string, unknown][] = [["missing", undefined], ["null", null], ["nested", { value: 7 }]];
  const erased = Object.fromEntries(values);
  assert(Object.hasOwn(erased, "missing"));
  assert(erased.missing === undefined);
  assert(erased["null"] === null);
  assert((erased.nested as { value: number }).value === 7);

  assert(Object.fromEntries([["inline", 6]]).inline === 6);

  const readonlyPairs: (readonly [string, number])[] = [["count", 5]];
  assert(Object.fromEntries(readonlyPairs).count === 5);
  assert(Object.fromEntries<number>([["explicit", 9]]).explicit === 9);
}
