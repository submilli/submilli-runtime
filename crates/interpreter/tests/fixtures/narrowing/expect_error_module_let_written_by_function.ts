// At the top level, what a module-level `let` was narrowed to, by its
// initializer or by a guard, doesn't survive a call to a function that
// assigns the variable.
// expect-error: expected `string`, got `number | string`
// expect-error: expected `string`, got `number | string`
// expect-error-count: 2
let current: string | number = "a";

function setNumber(): void {
  current = 5;
}

setNumber();
const initial: string = current;

if (typeof current === "string") {
  setNumber();
  const guarded: string = current;
  console.log(guarded);
}

function main(): void {
  console.log(initial, current);
}
