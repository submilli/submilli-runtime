// `super.method()` is a *direct call* to the parent's method body, skipping
// vtable dispatch — so an override can reuse the parent behavior even though
// dynamic dispatch on `this` would resolve back to the override.
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
  // Overrides Animal.speak, but defers to the parent body via super.speak().
  speak(): string {
    return super.speak() + " (a bark)";
  }
}

function main(): void {
  const rex = new Dog("Rex");
  // The override runs, and super.speak() reaches the *parent* body (not itself).
  assert(rex.speak() === "Rex makes a sound (a bark)");

  // Vtable dispatch through a base-typed reference still hits the override.
  const a: Animal = rex;
  assert(a.speak() === "Rex makes a sound (a bark)");

  // The base class on its own is unaffected.
  const cat = new Animal("Cat");
  assert(cat.speak() === "Cat makes a sound");
}
