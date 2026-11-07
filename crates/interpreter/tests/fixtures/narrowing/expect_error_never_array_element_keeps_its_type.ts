// An empty array typed `never[]` is a real value, so an array literal can't
// skip it the way it skips a dead `never` read: its type joins the element type.
// expect-error: expected `number[]`, got `(number | never[])[]`
const empty: never[] = [];
const mixed = [empty, 1];
const numbers: number[] = mixed;

function main(): void {
  console.log(numbers.length);
}
