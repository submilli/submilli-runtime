// `tag` uses `prefix`, so it exists only once the clause declaring `prefix` has
// run: another clause can't call it.
// expect-error: `tag` can't be used here: it uses `prefix`, which is declared in another `case` clause
// expect-error-count: 1
function pick(kind: number): string {
  switch (kind) {
    case 1:
      const prefix = "p:";
      function tag(text: string): string {
        return prefix + text;
      }
      return tag("a");
    default:
      return tag("b");
  }
}
function main(): void {}
