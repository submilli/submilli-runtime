// tsc types an unannotated `[]` that later gains elements or is reassigned from
// those writes (an evolving array). Submilli types a value where it is
// declared, so it asks for the element type there.
// expect-error: cannot infer the element type of `pushed` from an empty array
// expect-error: cannot infer the element type of `assigned` from an empty array
// expect-error: cannot infer the element type of `indexed` from an empty array
// expect-error-count: 3
function main(): void {
  const pushed = [];
  pushed.push(1);
  let assigned = [];
  assigned = [2];
  const indexed = [];
  indexed[0] = 3;
}
