// `x instanceof Foo` narrows `x` to `Foo` in the true branch (docs/classes.md §9).
// The narrowed binding gains access to the subclass's own members.
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak(): string {
    return this.name + " makes a sound";
  }
}

class Dog extends Animal {
  constructor(name: string) {
    super(name);
  }
  speak(): string {
    return this.name + " barks";
  }
  learn(trick: string): string {
    return this.name + " learned " + trick;
  }
}

function describe(animal: Animal): string {
  if (animal instanceof Dog) {
    // `animal` narrowed to Dog — `learn` is only on Dog.
    return animal.learn("sit");
  }
  return animal.speak();
}

function main(): void {
  const rex = new Dog("Rex");
  const cat = new Animal("Cat");

  assert(rex instanceof Dog);
  assert(!(cat instanceof Dog));

  assert(describe(rex) === "Rex learned sit");
  assert(describe(cat) === "Cat makes a sound");

  // A base-typed reference to a Dog still tests true.
  const a: Animal = rex;
  assert(a instanceof Dog);
  assert(describe(a) === "Rex learned sit");
}
