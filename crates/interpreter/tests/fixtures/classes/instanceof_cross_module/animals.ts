export class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak(): string {
    return this.name + " makes a sound";
  }
}

export class Dog extends Animal {
  constructor(name: string) {
    super(name);
  }
  learn(trick: string): string {
    return this.name + " learned " + trick;
  }
}
