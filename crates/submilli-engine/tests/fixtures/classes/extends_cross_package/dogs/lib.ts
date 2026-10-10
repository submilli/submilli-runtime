import { Animal } from "@test/animals";

// `Dog` is local to this package but extends an `Animal` imported from another
// package. Codegen reconstructs `Animal`'s rec group, lays `Dog`'s own field
// (`tricks`) after `Animal`'s field prefix, overrides `speak` in the inherited
// vtable slot, and adds `learn` in a new slot. `super(...)` calls Animal's
// imported ctor-init.
export class Dog extends Animal {
  private tricks: number;

  constructor(name: string) {
    super(name, "woof");
    this.tricks = 0;
  }

  // Overrides Animal.speak (same vtable slot, local body).
  speak(): string {
    return this.name + " barks";
  }

  learn(): number {
    this.tricks = this.tricks + 1;
    return this.tricks;
  }
}

// No explicit constructor: `Cat` inherits the imported `Animal`'s constructor
// signature across the package boundary (implicit-ctor inheritance).
export class Cat extends Animal {}
