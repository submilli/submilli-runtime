// An empty array typed `never[]` is a real value, so an array literal can't
// skip it the way it skips a dead `never` read: it fixes the element type.
// expect-error: expected `never[]` (matching first element), got `number`
const empty: never[] = [];
const mixed = [empty, 1];

function main(): void {
  console.log(mixed.length);
}
