// A class or shape field typed `never` holds no value, so a collection keyed by
// the type compiles and runs; nothing can build such a key.
class Box {
  v: never;
  n: number = 1;
  constructor(v: never) {
    this.v = v;
  }
}
interface Holder {
  v: never;
}

function main(): void {
  const boxes = new Set<Box>();
  const shapes = new Map<{ v: never }, number>();
  const holders = new Map<Holder, number>();
  assert(boxes.size === 0 && shapes.size === 0 && holders.size === 0, "empty collections");
  console.log(boxes.size, shapes.size, holders.size);
}
