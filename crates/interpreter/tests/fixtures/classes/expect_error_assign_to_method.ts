// Writing to a method has to name what's wrong. Reporting the member as missing
// and then suggesting the same name back reads as "retry verbatim".
//
// expect-error: cannot assign to method `m` on `C`
// expect-error: cannot assign to method `m` on `C`
// expect-error: cannot assign to method `m` on `C`
// expect-error: cannot assign to method `m` on `I`
// expect-error: cannot assign to method `m` on `I`
// expect-error: cannot assign to method `m` on `I`
// expect-error: methods are fixed at their declaration
interface I {
  m(a: number): number;
}

class C implements I {
  m(a: number): number {
    return a;
  }
}

export function main(): string {
  const c = new C();
  c.m = 5;
  c.m += 1;
  c.m++;

  const i: I = new C();
  i.m = 5;
  i.m += 1;
  i.m++;

  return "ok";
}
