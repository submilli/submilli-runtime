// expect-error: is always false
// Unrelated types: a `Cat` value can never be an instance of `Dog`, so the test
// is statically always false (docs/classes.md §9).
class Dog {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}

class Cat {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}

function main(): boolean {
  const c = new Cat("Tom");
  return c instanceof Dog;
}
