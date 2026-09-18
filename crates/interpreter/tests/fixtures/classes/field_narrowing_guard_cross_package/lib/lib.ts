export class Animal {
  speak(): string { return "animal"; }
}

export class Dog extends Animal {
  fetch(): string { return "stick"; }
}

export class Parent {
  value: Animal | null = null;
  reset(): void { this.value = new Animal(); }
}

export class Child extends Parent {
  value: Dog | null = new Dog();
}
