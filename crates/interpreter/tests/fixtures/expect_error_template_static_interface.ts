// expect-error: template-literal interpolation: `.toString()` not supported on `Console`
// A static-dispatch prelude value such as `console` has no object behind it, so
// it has no `toString` to interpolate.
function main(): void {
  console.log(`${console}`);
}
