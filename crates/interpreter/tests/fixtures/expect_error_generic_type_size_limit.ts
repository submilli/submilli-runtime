// expect-error: type is larger than the compiler limit of 65536 parts
// Each call's result type holds two copies of its argument's type, so r15
// has 65,535 parts and r16 is the first over the limit.
function pair<T>(x: T): { a: T; b: T } {
  return { a: x, b: x };
}
export function main(): number {
  const r0 = 1;
  const r1 = pair(r0);
  const r2 = pair(r1);
  const r3 = pair(r2);
  const r4 = pair(r3);
  const r5 = pair(r4);
  const r6 = pair(r5);
  const r7 = pair(r6);
  const r8 = pair(r7);
  const r9 = pair(r8);
  const r10 = pair(r9);
  const r11 = pair(r10);
  const r12 = pair(r11);
  const r13 = pair(r12);
  const r14 = pair(r13);
  const r15 = pair(r14);
  const r16 = pair(r15);
  return 1;
}
