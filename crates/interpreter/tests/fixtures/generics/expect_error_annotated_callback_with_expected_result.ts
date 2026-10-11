// An annotated callback binds its type parameter from its annotation, so with
// an expected result as well, a later argument that doesn't fit is reported
// once, at that argument, as tsc reports it.
// expect-error: expected `Dog`, got `Animal`
// expect-error-count: 1
class Animal {
  name: string = "a";
}

class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}

function f<T>(cb: (t: T) => void, v: T): T {
  return v;
}

function main(): void {
  const b: Dog = f((t: Dog) => {}, new Animal());
}
