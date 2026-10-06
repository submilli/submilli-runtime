// A callback whose parameter is a base class stands for one taking a subclass
// only once the subclass's type arguments are known: `Base<string>` can't take
// `Sub<T>` when a later argument binds `T` to `number`, as in tsc.
// expect-error: expected `Sub<T>`, got `Base<string>`
// expect-error-count: 1
class Base<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}

class Sub<T> extends Base<T> {
  more: number = 2;
}

function measure<T>(f: (sub: Sub<T>) => number, seed: T): number {
  return f(new Sub(seed));
}

function main(): void {
  console.log(measure((base: Base<string>) => base.value.length, 5));
}
