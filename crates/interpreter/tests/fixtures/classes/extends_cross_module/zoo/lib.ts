import { Animal } from "./animal";
export { Animal } from "./animal";

// `Dog` lives in a different module than its parent `Animal`, but in the same
// package — both compile into one Wasm module, so the in-module inheritance
// path handles the `extends`. The override slots into `Animal`'s vtable index.
export class Dog extends Animal {
  constructor(name: string) {
    super(name, "woof");
  }

  // Overrides Animal.speak.
  speak(): string {
    return this.name + " barks";
  }
}
