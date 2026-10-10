// An unannotated parameter beyond those the call passes has no type to take.
// expect-error: parameter `extra` is never passed an argument
// expect-error-count: 1
function main(): void {
  const xs = [1, 2];
  xs.forEach((v, i, all, extra) => {});
}
