class Animal { speak(): string { return "animal"; } }
class Dog extends Animal { fetch(): string { return "dog"; } }
class Box<T> { value: T; constructor(value: T) { this.value = value; } }
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Child<T> extends Parent {
  value: Box<T>;
  constructor(value: T) { super(); this.value = new Box<T>(value); }
  read(): Box<T> { return this.value; }
}
function inspect<T>(initial: T, replacement: unknown): void {
  const child = new Child<T>(initial);
  child.reset(replacement);
  const value = child.read();
}
export function main(): void {
  const dog = new Dog();
  inspect<Dog>(dog, new Box<Dog>(dog));
  let caught = false;
  try { inspect<Dog>(dog, new Box<Animal>(new Animal())); } catch(e) { caught = e instanceof TypeError; }
  assert(caught, "generic class payload validator forwards concrete arguments");
}
