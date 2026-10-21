// A type query in a function literal's parameter annotation takes its root's
// declared type: the literal may run after a captured `let` is reassigned, so
// the narrowing where it is written doesn't hold for it.
function main(): void {
  let v: string | number = "s";
  if (typeof v === "string") {
    const echo = (p: typeof v) => p;
    v = 5;
    assert(echo(1) === 1, "a number fits the declared type");
  }
}
