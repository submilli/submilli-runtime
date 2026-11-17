function id<T>(x: T): T { return x; }
let calls = 0;
function callCount(): number { return calls; }
function main(): number {
  if (+id(true) !== 1 || -id("3") !== -3 || -id(true) !== -1) return 1;
  if (+id([5]) !== 5 || !Object.is(-id([]), -0) || !Number.isNaN(+id({}))) return 2;
  const u: string | number = id("4");
  if (-u !== -4) return 4;
  const n: number | bigint = id(3n);
  if (-n !== -3n) return 5;
  const object = { valueOf: (): number => { calls++; return 7; } };
  if (-id(object) !== -7 || callCount() !== 1) return 6;
  const bigObject = { valueOf: (): bigint => { calls++; return 8n; } };
  if (String(-id(bigObject)) !== "-8" || callCount() !== 2) return 7;
  let threw = false;
  try { +id(bigObject); } catch (e: unknown) { threw = e instanceof TypeError; }
  if (!threw || callCount() !== 3) return 8;
  return 0;
}
