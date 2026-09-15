// Negative narrowing: `!(x instanceof Dog)` narrows the *false* branch to Dog,
// and a union member assignable to the class is dropped from the else side.
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}

class Dog extends Animal {
  constructor(name: string) {
    super(name);
  }
  bark(): string {
    return this.name + " barks";
  }
}

class Cat extends Animal {
  constructor(name: string) {
    super(name);
  }
  meow(): string {
    return this.name + " meows";
  }
}

// `pet` is a union of two sibling subclasses; instanceof discriminates them.
function sound(pet: Dog | Cat): string {
  if (!(pet instanceof Dog)) {
    // false branch of `!(...)` ⇒ pet is not a Dog ⇒ narrowed to Cat.
    return pet.meow();
  }
  // true branch of `!(...)` ⇒ pet is a Dog.
  return pet.bark();
}

function main(): void {
  const d = new Dog("Rex");
  const c = new Cat("Tom");
  assert(sound(d) === "Rex barks");
  assert(sound(c) === "Tom meows");
}
