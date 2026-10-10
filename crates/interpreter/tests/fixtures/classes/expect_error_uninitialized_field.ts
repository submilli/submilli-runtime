// expect-error: has no initializer and is not assigned in the constructor
class Broken {
  name: string;
  count: number;
  constructor(name: string) {
    this.name = name;
    // `count` is never assigned and has no initializer.
  }
}

function main(): void {
  const b = new Broken("x");
  assert(b.name === "x");
}
