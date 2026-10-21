// expect-error: expected `number`, got `string`
// A module-level declaration's annotation reads a type query's narrowing,
// as one in a function does: `g` holds a `number` after the assignment.
let g: string | number = "abc";
g = 5;
const named: typeof g = "y";

function main(): void {
  console.log(named);
}
