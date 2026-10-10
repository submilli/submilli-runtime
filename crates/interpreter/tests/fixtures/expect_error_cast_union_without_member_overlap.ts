// A cast between unions still needs one related pair of members; like tsc,
// these have none.
// expect-error-count: 3
// expect-error: cannot cast `string` to `number | boolean`
// expect-error: cannot cast `number | string` to `boolean | null`
// expect-error: cannot cast `{ a: number }` to `{ a: string } | { b: number }`
function f(text: string, mixed: string | number, shape: { a: number }): void {
  const a = text as number | boolean;
  const b = mixed as boolean | null;
  const c = shape as { a: string } | { b: number };
}

function main(): void {}
