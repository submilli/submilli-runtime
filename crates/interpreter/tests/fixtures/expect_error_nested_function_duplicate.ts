// expect-error: binding `k` is already declared in this scope
// expect-error-count: 1
function main(): void {
  function k(): void {}
  function k(): void {}
}
