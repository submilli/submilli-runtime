function mixedWrite(flag: boolean): number {
  let values = [];
  if (flag) values = [1];
  return values.push("s");
}
function concreteBranches(flag: boolean): number[] {
  let values = [];
  if (flag) values = [1]; else values = [2];
  values.push(3);
  return values;
}
function emptyBranches(flag: boolean): number[] {
  let values = [];
  if (flag) values = []; else values = [];
  values.push(3);
  return values;
}
let globalGuard = [];
function assignmentGuards(): void {
  let values = [];
  if (Array.isArray(values = [1])) {
    const first: number = values[0];
    assert(first === 1, "local assignment guard retains elements");
  }
  values.push(2);
  if (Array.isArray(globalGuard = [3])) {
    const first: number = globalGuard[0];
    assert(first === 3, "global assignment guard retains elements");
  }
  globalGuard.push(4);
}
function main(): void {
  assignmentGuards();
  assert(mixedWrite(false) === 1 && mixedWrite(true) === 2, "unresolved joins allow mutation");
  assert(concreteBranches(true).join(",") === "1,3", "all concrete assignments");
  assert(concreteBranches(false).join(",") === "2,3", "other concrete assignment");
  assert(emptyBranches(false)[0] === 3, "both branches restart evolution");
  let value;
  value = 1;
  if (typeof value === "string") {
    const impossible: never = value;
  }
  assert(value === 1, "ruled-out branch does not execute");
  const values = [];
  const before: number = values?.length;
  values.push(1);
  const after: number = values?.length;
  assert(before === 0 && after === 1, "optional length needs no element type");
}
