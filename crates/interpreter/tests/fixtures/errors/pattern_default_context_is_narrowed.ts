// expect-error: expected `number | "b"`, got `number | string`
// A pattern default is the context of its slot's value where the pattern
// stands, narrowing included: `k` is a `number` there, so `"b"` widens.
function pick(): "a" | number {
  return 1;
}

function main(): void {
  let k: "a" | number = pick();
  if (typeof k === "number") {
    const [x = k] = ["b"];
    const w: "b" | number = x;
  }
}
