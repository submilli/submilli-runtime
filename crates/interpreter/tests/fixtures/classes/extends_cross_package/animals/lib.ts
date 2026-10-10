export class Animal {
  name: string;
  private sound: string;

  constructor(name: string, sound: string) {
    this.name = name;
    this.sound = sound;
  }

  speak(): string {
    return this.name + " says " + this.sound;
  }

  // Not overridden by Dog — exercises an inherited method whose body lives in a
  // different package, reached through the subclass's vtable.
  describe(): string {
    return "an animal named " + this.name;
  }
}
