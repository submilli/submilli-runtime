// An object literal inside a closure inside a generic member. The shape the
// collector registers and the one codegen looks up have to agree, which means
// the types embedded in the literal must be erased alongside the node's own —
// they are the key both sides hash.
class Range<T> {
  private items: T[];

  constructor(items: T[]) {
    this.items = items;
  }

  iterator(): Iterator<T> {
    let i = 0;
    const items = this.items;
    return {
      next: (): IteratorResult<T> => {
        if (i < items.length) {
          const v = items[i];
          i = i + 1;
          return { done: false, value: v };
        }
        return { done: true };
      },
    };
  }
}

function main(): void {
  let total = 0;
  for (const x of new Range<number>([1, 2, 3])) {
    total = total + x;
  }
  assert(total === 6, "for-of over a user generic iterable");

  const words = new Range<string>(["a", "b"]);
  let joined = "";
  for (const w of words) {
    joined = joined + w;
  }
  assert(joined === "ab", "same class at a second instantiation");
}
