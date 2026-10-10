// An unrelated error in the initializer doesn't hide that `later`'s type, used
// above its declaration, names `base`, which is declared below that use.
// expect-error: expected `number`, got `string`
// expect-error: `later` is used by a function above its declaration, so its type can't name anything declared below that function
// expect-error-count: 2
const next = (): number => later + 1;
const base: number = 5;
let later: typeof base = "x";

function main(): void {
  console.log(String(next()));
}
