// `toFixed` rounds an exact tie up, as JS picks "the larger n", and
// `String` of a null held in a nullable union or `unknown` prints "null".
function main(): void {
  assert((2.25).toFixed(1) === "2.3", "an exact binary half rounds up");
  assert((0.125).toFixed(2) === "0.13", "an exact binary half rounds up");
  assert((-2.25).toFixed(1) === "-2.3", "a negative tie rounds away from zero");
  assert((2.5).toFixed(0) === "3", "a tie with no fraction digits");
  assert((9.995).toFixed(2) === "9.99", "a value just below the tie rounds down");
  assert((1.005).toFixed(2) === "1.00", "1.005 is below its tie in binary");

  const nullable: number | null = Math.random() > 2 ? 1 : null;
  assert(String(nullable) === "null", "a null in a nullable union");
  const opaque: unknown = nullable;
  assert(String(opaque) === "null", "a null in unknown");
  const voidResult: () => unknown = () => {};
  assert(String(voidResult()) === "undefined", "a void result read through unknown");
}
