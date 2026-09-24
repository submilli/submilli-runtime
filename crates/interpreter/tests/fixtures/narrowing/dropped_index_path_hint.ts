// expect-error: cannot preserve this guard
class Leaf { z: number | null = 3; }
class Element { y: Leaf | null = new Leaf(); }
class Holder { elems: Element[] = [new Element()]; }
function main(): number {
  const h = new Holder();
  if (h.elems[0].y !== null && h.elems[0].y.z !== null) {
    const n: number = h.elems[0].y.z;
    return n;
  }
  return 0;
}
