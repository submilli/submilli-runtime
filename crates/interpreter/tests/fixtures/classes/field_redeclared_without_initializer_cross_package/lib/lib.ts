export class Animal {
  speak(): string { return "generic"; }
}

export class Dog extends Animal {
  speak(): string { return "woof"; }
  fetch(): string { return "ball"; }
}

export class Holder {
  v: Animal | null | undefined = new Animal();
  tag?: string = "parent-tag";
}

// An intermediate the consumer extends without redeclaring in.
export class Middle extends Holder {
  extra: string = "mid";
}

// The reset is declared *inside* the library, so the consumer only instantiates
// a class whose layout it reconstructed rather than built.
export class LibReset extends Holder {
  v?: Dog;
}

export class GenHolder<T> {
  v?: T;
  constructor(v: T) {
    this.v = v;
  }
}
