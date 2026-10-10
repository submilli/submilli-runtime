class Animal { speak(): string { return "animal"; } }
class Dog extends Animal { fetch(): string { return "dog"; } }
class Base<T> { value: T | Animal | null = null; reset(value: T | Animal | null): void { this.value = value; } }
class Child<T> extends Base<T> {
  value: T | null = null;
  constructor(value: T | Animal | null) {
    super();
    this.reset(value);
    assert(this.value !== null, "constructor reads after initialization");
  }
}
function make<T>(value: T | Animal | null): Child<T> { return new Child<T>(value); }
function forward<U>(value: U | Animal | null): Child<U> { return make<U>(value); }
function deferred<T>(value: T | Animal | null): () => Child<T> { return () => forward<T>(value); }
class Factory<T> { make(value: T | Animal | null): Child<T> { return forward<T>(value); } }
class DogFactory extends Factory<Dog> {}
interface Node<T> { value: T; next: Node<T> | null; }
function rejects(action: () => void): void {
  let caught = false;
  try { action(); } catch(e) { caught = e instanceof TypeError; }
  assert(caught, "invalid generic constructor read is catchable");
}
export function main(): void {
  const dog = new Dog();
  assert(make<Dog>(dog).value === dog, "valid direct factory");
  assert(forward<Dog>(dog).value === dog, "valid nested generic call");
  assert(deferred<Dog>(dog)().value === dog, "valid escaping closure");
  const factory = new DogFactory();
  assert(factory.make(dog).value === dog, "valid inherited generic method");
  assert(make<Dog[]>([dog]).value !== null, "valid composite argument");
  const node: Node<Dog> = { value: dog, next: null };
  node.next = node;
  assert(make<Node<Dog>>(node).value !== null, "valid recursive argument");
  rejects(() => { const c = make<Dog>(new Animal()); });
  rejects(() => { const c = forward<Dog>(new Animal()); });
  rejects(() => { const c = deferred<Dog>(new Animal())(); });
  rejects(() => { const c = factory.make(new Animal()); });
  rejects(() => { const c = make<Dog[]>(new Animal()); });
}
