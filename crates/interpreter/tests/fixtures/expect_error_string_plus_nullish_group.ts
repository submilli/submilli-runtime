// expect-error: the receiver can be `undefined`
function main(): void {
  const match = "a".match(/a(b)?/);
  if (match !== null) console.log(match[1].length);
}
