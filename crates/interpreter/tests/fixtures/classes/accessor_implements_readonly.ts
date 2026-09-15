// A get-only accessor satisfies a `readonly` interface property.
interface HasId {
  readonly id: number;
}

class Entity implements HasId {
  private n: number;
  constructor(n: number) {
    this.n = n;
  }
  get id(): number {
    return this.n;
  }
}

function main(): void {
  const e: HasId = new Entity(42);
  assert(e.id === 42);
}
