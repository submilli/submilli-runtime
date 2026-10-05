// What a module-level `let` was narrowed to by its initializer or by a
// top-level assignment doesn't carry to the next top-level statement when a
// function assigns the variable, since that statement may call the function.
// expect-error: expected `string`, got `number | string`
// expect-error: expected `number`, got `number | string`
// expect-error-count: 2
let current: string | number = "a";

function setNumber(): void {
  current = 5;
}

function setString(): void {
  current = "b";
}

setNumber();
const initial: string = current;
current = 7;
setString();
const assigned: number = current;

function main(): void {
  console.log(initial, assigned);
}
