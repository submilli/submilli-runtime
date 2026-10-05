// An array literal's element type is the type every element fits, which need
// not be the first element's, as tsc's best common type: a one-parameter
// function fits a two-parameter function type, so both are held as that.
function main(): void {
  const fs = [(x: number) => x, (x: number, y: number) => x * y];
  assert(fs.map((f) => f(3, 4)).join(",") === "3,12", "two arities");

  const gs = [() => 7, (x: number) => x + 1, (x: number, y: number) => x - y];
  assert(gs.map((g) => g(5, 2)).join(",") === "7,6,3", "three arities, widest last");

  const hs = [(x: number, y: number) => x * y, (x: number) => x];
  assert(hs.map((h) => h(2, 5)).join(",") === "10,2", "widest first");
}
