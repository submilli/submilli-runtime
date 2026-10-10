// `rcv().f += v`, `a[i()] += v`, and their `++`/`--` spellings read and write
// through *one* evaluation of the reference expression, as JavaScript does.
// The typechecker synthesizes the read from the write's own receiver and index
// nodes, so without a guard codegen emits each of them twice — a doubled side
// effect, and a read and a write that can land on different objects.

class Counter {
  n: number = 10;
}

class Temp {
  private c: number = 10;
  get v(): number {
    return this.c;
  }
  set v(n: number) {
    this.c = n;
  }
}

let accCalls: number = 0;
const temp = new Temp();

function accRcv(): Temp {
  accCalls = accCalls + 1;
  return temp;
}

let rcvCalls: number = 0;
const shared = new Counter();

function rcv(): Counter {
  rcvCalls = rcvCalls + 1;
  return shared;
}

let order: string = "";

function first(): number {
  order = order + "f";
  return 0;
}

function second(): number {
  order = order + "g";
  return 1;
}

let mutated: Counter = new Counter();

function mutate(): number {
  mutated.n = 100;
  return 10;
}

let idxCalls: number = 0;

function idx(): number {
  idxCalls = idxCalls + 1;
  return 0;
}

const a: number[] = [10];
const b: number[] = [20];
let pickCalls: number = 0;

function pick(): number[] {
  pickCalls = pickCalls + 1;
  return pickCalls === 1 ? a : b;
}

interface Box {
  n: number;
  g: bigint;
}

let boxCalls: number = 0;
const sharedBox: Box = { n: 10, g: 1n };

function boxRcv(): Box {
  boxCalls = boxCalls + 1;
  return sharedBox;
}

class Holder {
  v: number = 1;
  bumpSelf(): void {
    this.v += 1;
  }
}

class Statics {
  static count: number = 0;
}

// A `finally` body is inlined once per exit path, so each inlining has to
// recompute the receiver rather than read the local the previous one filled.
function viaFinally(k: number): number {
  try {
    if (k === 0) {
      return 10;
    }
    if (k === 1) {
      throw new Error("x");
    }
    return 20;
  } finally {
    rcv().n += 1;
  }
}

function main(): void {
  rcvCalls = 0;
  rcv().n += 5;
  assert(rcvCalls === 1, "class field `+=` evaluates its receiver once");
  assert(shared.n === 15, "class field `+=` result");

  rcvCalls = 0;
  rcv().n++;
  assert(rcvCalls === 1, "class field `++` evaluates its receiver once");
  rcv().n--;
  rcv().n *= 2;
  rcv().n /= 2;
  rcv().n -= 6;
  assert(rcvCalls === 5, "every read-modify-write spelling evaluates once");
  assert(shared.n === 9, "class field arithmetic result");

  boxCalls = 0;
  boxRcv().n += 5;
  assert(boxCalls === 1, "object field `+=` evaluates its receiver once");
  boxRcv().n++;
  assert(boxCalls === 2, "object field `++` evaluates its receiver once");
  assert(sharedBox.n === 16, "object field arithmetic result");
  // Expression position goes through a different emitter than the statement
  // form, and an interface-typed receiver has to be stripped and cast there
  // too — its slot is `(ref null $Object)`, the write helper wants
  // `(ref $ObjectShape)`.
  boxCalls = 0;
  const prev = boxRcv().n++;
  assert(boxCalls === 1, "expression-position `++` evaluates its receiver once");
  assert(prev === 16 && sharedBox.n === 17, "expression-position `++` value");
  const prevBig = boxRcv().g++;
  assert(prevBig === 1n && sharedBox.g === 2n, "expression-position bigint `++`");

  // An accessor property has no data slot — the write dispatches the setter and
  // the read dispatches the getter, both through the one receiver.
  accCalls = 0;
  accRcv().v += 5;
  assert(accCalls === 1, "accessor `+=` evaluates its receiver once");
  assert(temp.v === 15, "accessor `+=` result");
  accRcv().v *= 2;
  assert(accCalls === 2, "accessor `*=` evaluates its receiver once");
  assert(temp.v === 30, "accessor `*=` result");

  idxCalls = 0;
  const c: number[] = [1, 2, 3];
  c[idx()] += 5;
  assert(idxCalls === 1, "index expression evaluates once");
  assert(c[0] === 6, "indexed `+=` result");
  c[idx()]++;
  assert(idxCalls === 2, "index expression evaluates once under `++`");
  assert(c[0] === 7, "indexed `++` result");

  // The read and the write must reach the same array: a receiver evaluated
  // twice read `b[0]` and wrote the sum into `a[0]`.
  pick()[0] += 1;
  assert(pickCalls === 1, "indexed receiver evaluates once");
  assert(a[0] === 11, "the write landed in the array the read came from");
  assert(b[0] === 20, "the second array was never touched");

  // An index whose own side effect is the increment: `i` must advance once, and
  // the write must land in the slot the pre-increment value named.
  const d: number[] = [10, 20, 30];
  let i: number = 0;
  d[i++] += 1;
  assert(i === 1, "`a[i++] += 1` advances `i` once");
  assert(d[0] === 11 && d[1] === 20, "`a[i++] += 1` wrote the slot `i` named");

  // The same shape through the other target kinds.
  const h = new Holder();
  h.bumpSelf();
  assert(h.v === 2, "`this.f += 1` inside a method");
  Statics.count += 1;
  Statics.count++;
  assert(Statics.count === 2, "static field read-modify-write");
  const strBox: { s: string } = { s: "a" };
  strBox.s += "b";
  assert(strBox.s === "ab", "string field `+=`");
  const bigBox: { g: bigint } = { g: 1n };
  bigBox.g += 2n;
  assert(bigBox.g === 3n, "bigint field `+=`");

  rcvCalls = 0;
  shared.n = 0;
  assert(viaFinally(0) === 10, "finally on an early return");
  assert(viaFinally(2) === 20, "finally on a fallthrough return");
  let threw = false;
  try {
    viaFinally(1);
  } catch (e) {
    threw = true;
  }
  assert(threw, "finally on a throw");
  assert(rcvCalls === 3, "one receiver evaluation per finally inlining");
  assert(shared.n === 3, "one increment per finally inlining");

  // Ordering: the target's index runs once and before the RHS, and the read
  // happens before a RHS that writes the very slot being read.
  const ord: number[] = [1, 2];
  ord[first()] += ord[second()];
  assert(order === "fg", "one evaluation each, target index first");
  assert(ord[0] === 3, "ordered compound result");
  mutated = new Counter();
  mutated.n = 5;
  mutated.n += mutate();
  assert(mutated.n === 15, "the read precedes the RHS's own write");

  // `%=` and `**=` complete the operator sweep, and carry their own
  // side-effecting index and receiver so they pin the evaluation count too.
  idxCalls = 0;
  const modArr: number[] = [10, 99];
  modArr[idx()] %= 3;
  assert(idxCalls === 1, "`%=` evaluates its index once");
  assert(modArr[0] === 1, "`%=` on an index");
  rcvCalls = 0;
  shared.n = 5;
  rcv().n **= 2;
  assert(rcvCalls === 1, "`**=` evaluates its receiver once");
  assert(shared.n === 25, "`**=` on a field");

  // Uint8Array shares the indexed-write path.
  idxCalls = 0;
  const u = Uint8Array.fromArray([1, 2, 3]);
  u[idx()] += 5;
  assert(idxCalls === 1, "Uint8Array index expression evaluates once");
  assert(u[0] === 6, "Uint8Array indexed `+=` result");
  const wrap = Uint8Array.fromArray([250]);
  wrap[0] += 10;
  assert(wrap[0] === 4, "Uint8Array compound wraps mod 256");
}
