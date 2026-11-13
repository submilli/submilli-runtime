// expect-error: cannot spread interface `Number` whose runtime backing is not a structural object
function main(): void {
  const n: Number = 3;
  const copy = { ...n };
  console.log(JSON.stringify(copy));
}
