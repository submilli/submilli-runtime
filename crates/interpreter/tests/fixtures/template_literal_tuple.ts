// A tuple is an array at run time, so `${t}` joins its positions with commas.
function main(): void {
  const pair: [number, string] = [1, "a"];
  const flags: readonly [number, boolean] = [2, true];
  const nested: [[number, string], number[]] = [[1, "x"], [2, 3]];
  const either: [number] | [string, string] = ["p", "q"];
  assert(`${pair}` === "1,a", "a tuple interpolates like an array");
  assert(`${flags}` === "2,true", "a readonly tuple interpolates too");
  assert(`${nested}` === "1,x,2,3", "nested tuples and arrays flatten");
  assert(`<${either}>` === "<p,q>", "a union of tuples interpolates");
}
