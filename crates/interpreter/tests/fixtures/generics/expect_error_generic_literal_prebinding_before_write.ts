// A field before an assigning field of a literal argument is typed with the
// narrowing in effect before the assignment, as tsc types it: `x` is still a
// `number` where `x.length` reads it.
// expect-error: cannot read field `length` on non-object type `number`
// expect-error-count: 1
function f<T>(o: { before: number; cb: (t: T) => void; after: T }): void {
  o.cb(o.after);
}

function main(): void {
  let x: string | number = 42;
  f({ before: x.length, cb: (t) => {}, after: (x = "now a string") });
}
