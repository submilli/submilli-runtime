// A guard on `record[key]` doesn't survive a write to the same entry spelled
// another way, as a field or with another key. A constant key naming a
// declared field reads as that field, so the field's write narrows it.
// expect-error: expected `string`, got `number`
// expect-error: expected `string`, got `number | string | undefined`
// expect-error-count: 2
type Pair = { a: string | number; b: string | number };

function byField(pair: Pair): string {
  const key = "a";
  if (typeof pair[key] === "string") {
    pair.a = 5;
    const text: string = pair[key];
    return text;
  }
  return "";
}

function byOtherKey(record: { [name: string]: string | number }): string {
  const key = "a";
  const other = "a";
  if (typeof record[key] === "string") {
    record[other] = 5;
    const text: string = record[key];
    return text;
  }
  return "";
}

function main(): void {
  console.log(byField({ a: "x", b: "y" }), byOtherKey({ a: "x" }));
}
