// A guard on a path through an element keeps narrowing that path: the read
// is rebuilt from the declared types at each step.
class Leaf { z: number | null = 3; }
class Item { y: Leaf | null = new Leaf(); }
class Holder { items: Item[] = [new Item()]; }

function depth(h: Holder): number {
  if (h.items[0].y !== null && h.items[0].y.z !== null) {
    const n: number = h.items[0].y.z;
    return n;
  }
  return 0;
}

function main(): void {
  assert(depth(new Holder()) === 3, "narrowed through an element");
}
