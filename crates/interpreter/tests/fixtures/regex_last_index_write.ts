// SUB-283: `lastIndex` is writable, as in tsc and JavaScript, and a `g` or `y`
// match starts from what was written.
function lastIndexOf(re: RegExp): number {
  return re.lastIndex;
}
function main(): void {
  const re = /a/g;
  assert(re.exec("aaa")!.index === 0 && re.lastIndex === 1);
  re.lastIndex = 2;
  assert(re.exec("aaa")!.index === 2 && re.lastIndex === 3);
  re.lastIndex = 0;
  assert(re.test("a") && re.lastIndex === 1);
  re.lastIndex += 5;
  assert(re.lastIndex === 6);
  assert(!re.test("aaa") && lastIndexOf(re) === 0);
  re.lastIndex++;
  assert(re.lastIndex === 1);
  const before = re.lastIndex--;
  assert(before === 1 && lastIndexOf(re) === 0);
  const sticky = /b/y;
  sticky.lastIndex = 1;
  assert(sticky.test("ab") && sticky.lastIndex === 2);
  const assigned = (sticky.lastIndex = 0);
  assert(assigned === 0 && sticky.lastIndex === 0);
  re.lastIndex = -1;
  assert(re.lastIndex === -1 && re.exec("a")!.index === 0);
}
