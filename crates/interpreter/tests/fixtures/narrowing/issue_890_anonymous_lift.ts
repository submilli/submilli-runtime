// expect-error-count: 3
// expect-error: expected 1 argument(s), got 0
// expect-error: this value is a type guard
// expect-error: rendering above is lossy
function isNum(x: number | string): x is number { return typeof x === "number"; }
function main(): void {
  const alias = isNum;
  alias();
  const arrow = (x: number | string): x is number => typeof x === "number";
  arrow();
  const maybe = Math.random() > 0.5 ? arrow : null;
  maybe?.();
}
