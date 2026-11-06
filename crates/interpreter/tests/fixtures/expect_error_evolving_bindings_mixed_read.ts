// expect-error: cannot infer the type of `values` at this read
// expect-error: cannot infer the type of `grown` at this read
function mixed(flag: boolean): number[] {
  let values = [];
  if (flag) values = [1];
  return values;
}
function mixedAfterPush(flag: boolean): (number | string)[] {
  let grown = [];
  if (flag) grown = [1];
  grown.push("s");
  return grown;
}
function main(): void {}
