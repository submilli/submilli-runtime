// A subclass with no explicit constructor inherits the parent's constructor
// signature and forwards all arguments to `super(...)`.
class Animal {
  name: string;
  legs: number;
  constructor(name: string, legs: number) {
    this.name = name;
    this.legs = legs;
  }
  describe(): string {
    return this.name + " has " + this.legs.toString() + " legs";
  }
}

// No `constructor` declared — `new Dog("Rex", 4)` forwards to Animal's ctor.
class Dog extends Animal {
  bark(): string {
    return this.name + " barks";
  }
}

function main(): void {
  const rex = new Dog("Rex", 4);
  assert(rex.name === "Rex");
  assert(rex.legs === 4);
  assert(rex.describe() === "Rex has 4 legs");
  assert(rex.bark() === "Rex barks");
}
