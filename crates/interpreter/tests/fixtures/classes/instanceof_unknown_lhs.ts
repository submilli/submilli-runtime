// `instanceof` with an `unknown` LHS: class instances test true; non-class
// values (strings, boxed numbers, structural objects, arrays, null) fall out
// false through the non-class-vtable exit.
class Thing {
  tag: string;
  constructor() {
    this.tag = "thing";
  }
}

function check(x: unknown): boolean {
  return x instanceof Thing;
}

function main(): void {
  assert(check(new Thing()));
  assert(!check("thing"));
  assert(!check(42));
  assert(!check({ tag: "thing" }));
  assert(!check([1, 2]));
  assert(!check(null));
}
