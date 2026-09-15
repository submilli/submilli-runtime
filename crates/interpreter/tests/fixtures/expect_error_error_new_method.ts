// expect-error: class `Error` has no static member `new`
function main(): string {
  const e = Error.new("boom");
  return e.message;
}
