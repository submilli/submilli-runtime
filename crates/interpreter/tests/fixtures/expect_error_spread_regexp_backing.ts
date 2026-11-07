// expect-error: cannot spread interface `RegExp` whose runtime backing is not a structural object
function main(): void {
  const original = /a/g;
  original.lastIndex = 1;
  const copy = { ...original };
  console.log(JSON.stringify(copy));
}
