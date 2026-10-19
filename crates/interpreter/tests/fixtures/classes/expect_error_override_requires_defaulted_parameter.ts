// expect-error: a call through the inherited method may omit `y`
// An omitted argument takes the default of the method that runs, so an override
// that requires `y` would receive undefined through a `Base`-typed call.
class Base {
  scale(a: number, y: number = 10): number {
    return a * y;
  }
}
class Derived extends Base {
  scale(a: number, y: number): number {
    return a * y * 2;
  }
}
function main(): void {
  const b: Base = new Derived();
  console.log(b.scale(1));
}
