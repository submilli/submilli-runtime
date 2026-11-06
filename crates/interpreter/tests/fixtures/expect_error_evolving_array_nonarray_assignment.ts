// expect-error: expected `unknown[]`
let globalValues = [];
globalValues = 1;
function main(): void {
  let values = [];
  values = "not an array";
  values = undefined;
  let changing;
  changing = [];
  changing = 1;
  // Uninitialized let is allowed to change from array to scalar; only this use fails.
  changing.push(1);
}
