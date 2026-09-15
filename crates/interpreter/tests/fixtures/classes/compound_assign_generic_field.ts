// Read-modify-write on a generic class's fields: the target's type comes back
// with the declaring class's generics substituted from the receiver's type
// arguments, so `+=` sees `number`/`string`, never the bare type variable.
class Box<T> {
  value: T;
  count: number = 0;

  constructor(v: T) {
    this.value = v;
  }

  tick(): void {
    this.count += 1;
  }
}

class Pair<A, B> {
  left: A;
  right: B;

  constructor(l: A, r: B) {
    this.left = l;
    this.right = r;
  }
}

// The type argument is bound by `extends`, not at the use site.
class NumBox extends Box<number> {
  boost(): void {
    this.value += 100;
    this.value++;
  }
}

function main(): void {
  const bn = new Box<number>(5);
  bn.value += 1;
  bn.value++;
  assert(bn.value === 7);

  bn.count += 2;
  bn.tick();
  assert(bn.count === 3);

  const bs = new Box<string>("a");
  bs.value += "b";
  assert(bs.value === "ab");

  const p = new Pair<number, string>(1, "x");
  p.left *= 10;
  p.right += "!";
  assert(p.left === 10);
  assert(p.right === "x!");

  const nb = new NumBox(1);
  nb.boost();
  assert(nb.value === 102);
  const old = nb.value++;
  assert(old === 102);
  assert(nb.value === 103);
}
