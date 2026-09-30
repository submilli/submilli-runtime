// expect-error: unreachable code
// expect-error-count: 1
function pick(): number {
  return 1;
  const after = 2;
}
