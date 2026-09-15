// expect-error: is not a class
// `instanceof` requires a class on the right-hand side; interfaces aren't runtime
// types in v1 (docs/classes.md §9).
interface Named {
  name: string;
}

class Dog {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}

function main(): boolean {
  const d: Named = new Dog("Rex");
  return d instanceof Named;
}
