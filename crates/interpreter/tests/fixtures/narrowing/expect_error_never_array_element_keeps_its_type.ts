// An empty array typed `never[]` is a real value, so it can't sit in an
// array of numbers just because it comes first.
// expect-error: expected `number` (matching first element), got `never[]`
const empty: never[] = [];
const mixed = [empty, 1];

function main(): void {
  console.log(mixed.length);
}
