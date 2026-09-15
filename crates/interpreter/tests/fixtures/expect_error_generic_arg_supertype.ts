// The relaxation is one-directional: a *supertype* argument against a bound
// type parameter is still rejected, or a `Map<Sub, V>` could be written
// through as if it held `Base`s.
// expect-error: got `Base`
class Base { x: number = 1; }
class Sub extends Base { y: number = 2; }

function main(): void {
  const m = new Map<Sub, number>();
  m.set(new Base(), 3);
}
