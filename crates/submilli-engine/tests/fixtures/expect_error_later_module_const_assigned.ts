// A later module-level `const` can't be assigned from a function above it.
// expect-error: cannot assign to const binding `limit`
// expect-error-count: 1
const reset = (): void => {
  limit = 2;
};
const limit = 1;
function main(): void {}
