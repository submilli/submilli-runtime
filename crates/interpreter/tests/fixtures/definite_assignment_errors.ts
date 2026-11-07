// expect-error: variable `direct` is used before being assigned
// expect-error: variable `branch` is used before being assigned
// expect-error: variable `loop` is used before being assigned
// expect-error: variable `self` is used before being assigned
// expect-error: variable `increment` is used before being assigned
// expect-error: variable `compound` is used before being assigned
// expect-error: variable `caught` is used before being assigned
function check(flag: boolean): void {
  let direct: number;
  console.log(direct);
  let branch: string;
  if (flag) { branch = "yes"; }
  console.log(branch);
  let loop: number;
  while (flag) { loop = 1; }
  console.log(loop);
  let self: number;
  self = self;
  let increment: number;
  increment++;
  let compound: number;
  compound += 1;
  let caught: number;
  try { caught = 1; } catch { console.log(caught); }
}
function main(): void { check(false); }
