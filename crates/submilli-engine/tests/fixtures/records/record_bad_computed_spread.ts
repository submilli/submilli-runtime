// expect-error: expected
function build(key: string): Record<string, number> {
  const strings: Record<string, string> = { bad: "oops" };
  return { [key]: 1, ...strings };
}
function main(): void {
  const numbers = build("good");
  assert(numbers.bad === 0);
}
