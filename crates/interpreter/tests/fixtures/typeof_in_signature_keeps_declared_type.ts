// A type query in a signature takes its root's declared type: the function
// may run after a captured `let` is reassigned, so the narrowing where it is
// written doesn't hold for it.
function main(): void {
  let v: string | number = "s";
  if (typeof v === "string") {
    const echo = (p: typeof v) => p;
    const back = function (p: number): typeof v {
      return p;
    };
    function inner(p: typeof v): typeof v {
      return p;
    }
    v = 5;
    assert(echo(1) === 1, "a parameter");
    assert(back(2) === 2, "a return type");
    assert(inner(3) === 3, "a nested declaration");
  }
}
