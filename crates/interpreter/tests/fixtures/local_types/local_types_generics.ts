// A local type may use the type parameters of its enclosing function, method
// or class, directly or through another local type.

function wrap<T>(x: T): T[] {
  interface Box { v: T; }
  // `Pair` uses `T` only through `Box`; its own `T` shadows the outer one.
  interface Pair<T> { box: Box; other: T; }
  type Boxes = Box[];
  const p: Pair<string> = { box: { v: x }, other: "s" };
  const boxes: Boxes = [p.box];
  return boxes.map(b => b.v);
}

function firstOf<T>(items: T[], fallback: T): T {
  type Maybe = T | undefined;
  const found: Maybe = items.length > 0 ? items[0] : undefined;
  // A nested function's signature in the same block sees the local type.
  function orElse(m: Maybe): T {
    return m === undefined ? fallback : m;
  }
  return orElse(found);
}

class Stack<T> {
  private items: T[] = [];
  push(item: T): void {
    this.items.push(item);
  }
  peekAll(): T[] {
    interface Entry { item: T; index: number; }
    const entries: Entry[] = this.items.map((item, index) => ({ item, index }));
    return entries.map(e => e.item);
  }
}

function main(): void {
  const nums = wrap(5);
  assert(nums[0] === 5, "Box<number>");
  const strs = wrap("a");
  assert(strs[0] === "a", "Box<string>");
  assert(firstOf([3, 4], 0) === 3, "first element");
  assert(firstOf<string>([], "none") === "none", "fallback");
  const stack = new Stack<string>();
  stack.push("x");
  stack.push("y");
  assert(stack.peekAll().join("") === "xy", "class type parameter in a method's local type");
  const lengths = ["ab", "c"].map(s => {
    interface Measured { text: string; size: number; }
    const m: Measured = { text: s, size: s.length };
    return m.size;
  });
  assert(lengths.join(",") === "2,1", "local type in an arrow body");
}
