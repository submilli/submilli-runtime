// expect-error: cannot spread interface `Map` whose runtime backing is not a structural object
function main(): void {
  const entries = new Map<string, number>();
  const copy = { ...entries };
  console.log(JSON.stringify(copy));
}
