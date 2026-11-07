// An array element inferred again alone sees the flow from before it, so an
// evolving array read before its first `push` in that element has no type yet,
// as in tsc.
// expect-error: cannot infer the type of `xs` at this read
// expect-error-count: 2
function main(): void {
  let xs = [];
  const rows = [[true], [xs, xs.push(1)]];
  console.log(rows.length);
}
