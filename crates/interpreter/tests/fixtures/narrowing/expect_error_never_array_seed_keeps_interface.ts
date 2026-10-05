// A `never[]` first element gives way only to a later array: an interface
// it fits by its fields would read the array as an object.
// expect-error: expected `never[]` (matching first element), got `Sized`
interface Sized {
  length: number;
}
const empty: never[] = [];
const sized: Sized = { length: 3 };
const mixed = [empty, sized];

function main(): void {
  console.log(mixed.length);
}
