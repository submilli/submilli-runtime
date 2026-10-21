// A type query in a signature takes only a narrowing that survives into the
// function, as in tsc: it may run after a captured `let` is reassigned, so a
// narrowing of one doesn't hold for it, while one of a `const` does.
function pick(): string | number {
  return "abc";
}
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
  const fixed = pick();
  if (typeof fixed === "string") {
    const length = (s: typeof fixed) => s.length;
    assert(length(fixed) === 3, "a const root stays narrowed");
  }
}
