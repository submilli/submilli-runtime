// A defaulted parameter typed as a union of number literals can be written,
// incremented and read back, in functions, arrows and methods.
function f(x: 1 | 2 = 1): number {
  x = 2;
  return x;
}
const g = (y: 1 | 2 = 1): number => {
  y++;
  return y;
};
class C {
  m(z: 1 | 2 | undefined = 1): number {
    z = 2;
    return z;
  }
}
enum Mode { Off, On }
function toggle(mode: Mode = Mode.Off, flag: true | undefined = true): number {
  const flip = (): void => {
    mode = Mode.On;
  };
  flip();
  return mode === Mode.On && flag === true ? 1 : 0;
}
function main(): void {
  assert(toggle() === 1, "an enum default written by a closure");
  assert(f() === 2 && f(1) === 2, "a write");
  assert(g() === 2 && g(2) === 3, "an increment");
  assert(new C().m() === 2, "a method");
}
