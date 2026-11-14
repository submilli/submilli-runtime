// A JSON number past the double range reads as an infinity, as in JavaScript;
// it is not a RangeError.
function main(): void {
  const big: unknown = JSON.parse("1e400");
  assert(big === Infinity, "1e400 reads as Infinity");
  const small: unknown = JSON.parse("-1e400");
  assert(small === -Infinity, "-1e400 reads as -Infinity");
  const items = JSON.parse("[1e999]") as number[];
  assert(items.length === 1 && items[0] === Infinity, "[1e999] holds Infinity");
  const max: unknown = JSON.parse("1.7976931348623157e308");
  assert(max === Number.MAX_VALUE, "the largest double stays finite");
  const tiny: unknown = JSON.parse("1e-400");
  assert(tiny === 0, "an underflow reads as 0");
}
