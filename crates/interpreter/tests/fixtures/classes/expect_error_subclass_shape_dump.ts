// A subclass that declares nothing of its own still has everything it inherits;
// the shape dump has to show it rather than printing an empty body.
// expect-error: field `nope` does not exist on `Sub`
// expect-error: class Sub extends Base
// expect-error: x: number;  // from Base
// expect-error: foo(): number;  // from Base
class Base {
  x: number = 1;
  foo(): number {
    return this.x;
  }
}

class Sub extends Base {}

function main(): void {
  const s = new Sub();
  console.log(`${s.nope}`);
}
