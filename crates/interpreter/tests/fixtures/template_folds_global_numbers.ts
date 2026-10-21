// A template folds `Infinity` and `NaN` to the text they print, as TypeScript
// folds the globals; a local of either name is folded by its own value.
function shadowed(): string {
  const Infinity = 5;
  const five: "5" = `${Infinity}`;
  return five;
}

function main(): void {
  const infinite: "Infinity" = `${Infinity}`;
  const notANumber: "NaN" = `${NaN}`;
  const negative: "-Infinity" = `${-Infinity}`;
  const wrapped: "abcInfinitydef" = `abc${+Infinity}def`;
  assert(infinite === "Infinity" && notANumber === "NaN", "the globals");
  assert(negative === "-Infinity" && wrapped === "abcInfinitydef", "signed");
  assert(shadowed() === "5", "a shadowing local");
}
