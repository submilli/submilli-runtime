// No `=>` follows, so this really is an unclosed group and keeps the group's own
// diagnostic — the function-type recovery must not swallow it.
// expect-error: expected `)` to close a parenthesized type
type Bad = (number;

function main(): void {
  console.log("unreachable");
}
