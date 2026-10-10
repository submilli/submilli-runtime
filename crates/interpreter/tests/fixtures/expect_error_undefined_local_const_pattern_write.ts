// expect-error: cannot assign to const binding `tail`
function main(): void {
  const { head, ...tail } = { head: 0, value: 42 };
  tail = { value: 7 };
}
