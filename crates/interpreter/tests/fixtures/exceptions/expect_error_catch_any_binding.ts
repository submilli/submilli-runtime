// A catch binding annotated `any` is refused, and the help names the catch
// forms that compile rather than `unknown`, which a catch binding also refuses.
// expect-error-count: 1
// expect-error: `any` is not supported
// expect-error: write `catch (e)` to catch every thrown error

function main(): void {
  try {
    throw new RangeError("r");
  } catch (e: any) {
    console.log(e.message);
  }
}
