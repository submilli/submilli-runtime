// expect-error: expected `string`, got `string | null`
// expect-error: unreachable code
// Code after a `return` never runs, so it reads declared types, as in
// TypeScript: the guard before the `return` no longer narrows `text` there.
function length(text: string | null): number {
  if (text === null) return 0;
  return text.length;
  const kept: string = text;
}

function main(): void {
  console.log(length("abc"));
}
