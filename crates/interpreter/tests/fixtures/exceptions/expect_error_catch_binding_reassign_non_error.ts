// expect-error: expected `Error`, got `number`
// The binding stays `Error` (spec: `catch (e)` means `catch (e: Error)`), so
// only an `Error` can be assigned to it.
function main(): void {
  try {
    throw new Error("x");
  } catch (e) {
    e = 1;
  }
}
