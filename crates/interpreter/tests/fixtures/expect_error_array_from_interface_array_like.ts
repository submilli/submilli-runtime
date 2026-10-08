// expect-error: `AL` has no number index signature, so it isn't an `ArrayLike`
// expect-error-count: 1
// TypeScript's `ArrayLike` has a number index signature, which an interface
// doesn't have implicitly (TS2769).
interface AL {
  length: number;
}

function main(): void {
  const al: AL = { length: 2 };
  console.log(Array.from(al, (v: number, i: number) => v + i).join(","));
}
