// expect-error: expected `never`, got `number`
// expect-error: expected `1`, got `number`
// expect-error: expected `"a"`, got `string`
const fixedGlobal: never[] = [];
let globalArray = [];
globalArray = fixedGlobal;
globalArray.push(1);
function main(): void {
  const fixed: never[] = [];
  let local = [];
  local = fixed;
  local.push(1);
  let uninitialized;
  uninitialized = fixed;
  uninitialized.push(1);
  const literal: 1 = 1;
  const values = [];
  values.push(literal);
  const exact: 1 = values[0];
  const letter: "a" = "a";
  const indexed = [];
  indexed[0] = letter;
  const exactLetter: "a" = indexed[0];
}
