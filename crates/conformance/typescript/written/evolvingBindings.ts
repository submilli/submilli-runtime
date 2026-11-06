// Written for SUB-2101: flow types of uninitialized variables and evolving arrays.
// @strict: true
// @target: es2022
function arrays(flag: boolean): void {
  const loop = [];
  for (const name of ["a", "b"]) loop.push(name);
  const strings: string[] = loop;
  const mixed = [];
  mixed.push(1);
  const first: number = mixed[0];
  mixed.unshift("x");
  const both: (number | string)[] = mixed;
  const branch = [];
  if (flag) branch.push("yes");
  const maybeEmpty: string[] = branch;
  let reassigned = [];
  reassigned = [2];
  const numbers: number[] = reassigned;
  const indexed = [];
  indexed[0] = 3;
  const indexValue: number = indexed[0];
  const empty = [];
  const length: number = empty.length;
}
function variables(flag: boolean): void {
  let result;
  try { result = "ok"; } catch (error) { return; }
  const text: string = result;
  let mixed;
  if (flag) mixed = "yes"; else mixed = 7;
  const union: string | number = mixed;
  let maybe;
  if (flag) maybe = 1;
  const optional: number | undefined = maybe;
}
function rejectedReads(): void {
  const values = [];
  const read = (): number[] => values;
  values.push(1);
  read();
}
function main(): void {}
