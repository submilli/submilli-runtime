// expect-error: function `pick` does not return a value on all paths
// expect-error-count: 1
function pick(flag: boolean): number {
  if (flag) {
    return 1;
  }
}
