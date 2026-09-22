// expect-error: unresolved identifier `missing`
function main(): void {
  const x: typeof missing = 1;
}
