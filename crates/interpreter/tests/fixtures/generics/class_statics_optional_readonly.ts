// Statics, `readonly`, and optional (`?`) fields on a generic class. Statics are
// namespaced module artifacts with no `T` binding; an optional `T` field reads
// as `T | undefined` before it is assigned.
class Box<T> {
  static readonly kind: string = "box";
  static describe(): string {
    return Box.kind + "!";
  }

  readonly created: number;
  v?: T;
  note?: string;

  constructor(created: number) {
    this.created = created;
  }

  put(x: T): void {
    this.v = x;
  }
}

class NumBox extends Box<number> {}

function main(): void {
  assert(Box.kind === "box", "static readonly field on a generic class");
  assert(Box.describe() === "box!", "static method reading its own static");
  assert(NumBox.kind === "box", "static inherited by a subclass at concrete args");

  const b = new Box<string>(7);
  assert(b.created === 7, "readonly field on a generic class");
  assert(b.v === undefined, "unassigned optional T field reads undefined");
  assert(b.note === undefined, "unassigned optional non-generic field reads undefined");
  b.put("q");
  assert(b.v !== undefined && b.v.length === 1, "optional T field narrows after assignment");
}
