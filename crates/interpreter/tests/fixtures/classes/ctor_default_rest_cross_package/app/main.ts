import {
  Bag,
  Counter,
  Peer,
  PeerBag,
  Props,
  Quiet,
  Shade,
  Tagged,
  Themed,
  Tone,
} from "@test/lib";

class LocalCounter extends Counter {
  constructor(n: number) {
    super("local", n, n);
  }
  doubled(): number {
    return this.total * 2;
  }
}

class Silent extends Counter {
  constructor() {
    super();
  }
  viaSuper(): number {
    return super.sum();
  }
}

class Forward extends Counter {}

class LocalBag extends Bag<string> {}

function main(): void {
  const c = new Counter();
  assert(c.total === 0 && c.label === "lib", "imported ctor: default filled, rest empty");
  const c2 = new Counter("app", 1, 2, 3);
  assert(c2.total === 6 && c2.label === "app", "imported ctor: rest tail packed");
  const c3 = new Counter("one");
  assert(c3.total === 0 && c3.label === "one", "imported ctor: explicit fixed, empty rest");

  assert(c2.sum() === 16, "imported method: omitted default");
  assert(c2.sum(1, 2, 3) === 12, "imported method: rest tail");

  const p = new Props("a");
  assert(p.scale === 3 && p.tags.length === 0, "imported defaulted parameter properties");
  const p2 = new Props("b", 5, ["x"]);
  assert(p2.scale === 5 && p2.tags[0] === "x", "imported parameter properties, explicit args");

  const l = new LocalCounter(4);
  assert(l.doubled() === 16, "local `super(...)` past an imported default into the rest slot");

  const s = new Silent();
  assert(s.total === 0 && s.label === "lib", "local `super()` fills the imported default");
  assert(s.viaSuper() === 10, "`super.method()` on an imported parent fills its default");

  const f = new Forward("fwd", 7);
  assert(f.total === 7, "local implicit ctor forwards an imported default+rest signature");

  const q = new Quiet();
  assert(q.label === "quiet" && q.total === 0, "producer-side subclass omitting the rest slot");

  const b = new Bag<number>(1, 2, 3);
  assert(b.first === 1 && b.count === 2, "imported generic ctor rest tail");
  const t = new Tagged<number>(9);
  assert(t.tag === "none", "imported generic ctor default");

  const lb = new LocalBag("a", "b");
  assert(lb.count === 1, "local subclass of an imported generic rest ctor");

  const th = new Themed();
  assert(th.tone === Tone.Loud, "imported number enum default across the boundary");
  assert(th.shade === Shade.Deep, "imported string enum default across the boundary");
  assert(th.peer === null, "imported `null` default in a class-typed slot");
  assert(typeof th.any === "number" && th.any === 5, "imported default boxed into `unknown`");
  assert(typeof th.either === "number" && th.either === 7, "imported default boxed into a union slot");
  const th2 = new Themed(Tone.Soft, Shade.Pale, new Peer(1), "s", "t");
  assert(th2.peer !== null && th2.peer.n === 1, "explicit args past the imported defaults");

  const pb = new PeerBag("peers", new Peer(2), new Peer(3));
  assert(pb.total === 5 && pb.label === "peers", "imported class-typed rest elements");
  assert(new PeerBag().total === 0, "imported class-typed rest, default label, empty tail");
}
