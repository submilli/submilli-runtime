// A template folds `Infinity` and `NaN` to the text they print, as TypeScript
// folds the globals; a local of either name is folded by its own value.
function shadowed(): string {
  const Infinity = 5;
  const five: "5" = `${Infinity}`;
  return five;
}

// A `case` label naming either global stays an expression of type `number`,
// as tsc types it: two `case NaN` labels don't clash, and neither matches.
function label(x: number): string {
  switch (x) {
    case NaN:
      return "nan";
    case NaN:
      return "nan again";
    case Infinity:
      return "infinite";
  }
  return "finite";
}

function main(): void {
  const infinite: "Infinity" = `${Infinity}`;
  const notANumber: "NaN" = `${NaN}`;
  const negative: "-Infinity" = `${-Infinity}`;
  const wrapped: "abcInfinitydef" = `abc${+Infinity}def`;
  assert(infinite === "Infinity" && notANumber === "NaN", "the globals");
  assert(negative === "-Infinity" && wrapped === "abcInfinitydef", "signed");
  assert(shadowed() === "5", "a shadowing local");
  assert(label(NaN) === "finite" && label(Infinity) === "infinite", "case labels");
}
