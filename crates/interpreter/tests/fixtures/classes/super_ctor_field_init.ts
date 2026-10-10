// `super(...)` in a subclass constructor calls the parent constructor *body*,
// initializing the parent-declared fields in place on the already-allocated
// instance before the subclass runs its own field assignments.
class Animal {
  name: string;
  private sound: string;
  constructor(name: string, sound: string) {
    this.name = name;
    this.sound = sound;
  }
  speak(): string {
    return this.name + " says " + this.sound;
  }
}

class Dog extends Animal {
  private tricks: number;
  constructor(name: string) {
    super(name, "woof");
    this.tricks = 3;
  }
  trickCount(): number {
    return this.tricks;
  }
}

function main(): void {
  const rex = new Dog("Rex");
  // Inherited public field, initialized by the parent body via super(...).
  assert(rex.name === "Rex");
  // Inherited method reads the parent-private field set by super(...).
  assert(rex.speak() === "Rex says woof");
  // Subclass's own field, set after super(...).
  assert(rex.trickCount() === 3);

  // A direct base-class instance still constructs and behaves the same.
  const a = new Animal("Cat", "meow");
  assert(a.speak() === "Cat says meow");
}
