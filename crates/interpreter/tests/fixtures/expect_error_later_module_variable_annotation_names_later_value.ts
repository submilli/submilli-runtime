// `later` is used above its declaration, where its `typeof base` annotation
// can't be resolved yet: `base` is declared below that use too. (tsc accepts
// this.)
// expect-error: `later` is used by a function above its declaration, so its type can't name anything declared below that function
// expect-error-count: 1
const next = (): number => later + 1;
const base: number = 5;
let later: typeof base = 7;

function main(): void {
  console.log(String(next()));
}
