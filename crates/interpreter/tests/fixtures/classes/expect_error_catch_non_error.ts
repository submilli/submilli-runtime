// expect-error: must be `Error`
function main(): void {
  try {
    throw new Error("x");
  } catch (e: string) {
    assert(false);
  }
}
