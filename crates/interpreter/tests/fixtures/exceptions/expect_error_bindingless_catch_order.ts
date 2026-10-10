// expect-error: duplicate `catch` clause for `Error`
function main(): void {
  try { throw new Error("x"); } catch {} catch (e: Error) {}
}
