// `later` and `step` are used above their declarations, where their types can't
// be resolved yet: each names `base`, which is declared below that use too.
// (tsc accepts this.)
// expect-error: is used by a function above its declaration, so its type can't name anything declared below that function
// expect-error-count: 2
const next = (): number => later + step(1);
const base: number = 5;
let later: typeof base = 7;
const step = (n: typeof base): number => n + 1;

function main(): void {
  console.log(String(next()));
}
