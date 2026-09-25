import {
  Sized,
  Bag,
  MaybeName,
  Perimeter,
  bump,
  chainArea,
  chainNick,
  dump,
  hiddenSized,
  inc,
  onlyWrite,
  readArea,
  readNick,
  readNote,
  readPerimeter,
  same,
  writeArea,
  writeNote,
} from "@test/shape";

class ByAccessor implements Sized {
  private side: number = 3;
  get area(): number { return this.side * this.side; }
  set area(v: number) { this.side = v; }
}

class Plain implements Sized { area: number = 1; }

class BagAcc implements Bag {
  tag: string = "t";
  private n: string = "hidden";
  get note(): string { return this.n; }
  set note(v: string) { this.n = v; }
}

class BagPlain implements Bag { tag: string = "t"; }

// A required member backed by a getter with no setter must still reach the
// accessor branch: the absent-property fallback widens `null`, which a
// non-nullable `number` cannot hold.
class AppPerimeter implements Perimeter {
  private side: number = 2;
  get perimeter(): number { return this.side * 4; }
}

// A read-only *optional* member backed by a getter: the absent-member fallback
// and the accessor branch both produce `string | null`, so only the accessor
// name scan tells them apart.
class NickAcc implements MaybeName {
  get nick(): string {
    return "acc-nick";
  }
}

class NickNone implements MaybeName {}

class NickField implements MaybeName {
  readonly nick?: string = "field-nick";
}

function main(): void {
  assert(readArea(new Plain()) === 1, "data field across the boundary");

  const a = new ByAccessor();
  assert(readArea(a) === 9, "accessor read across the boundary");
  writeArea(a, 4);
  assert(readArea(a) === 16, "accessor write across the boundary");

  const b = new BagAcc();
  assert(readNote(b) === "hidden", "optional accessor read across the boundary");
  writeNote(b, "w2");
  assert(readNote(b) === "w2", "optional accessor write across the boundary");

  // An absent optional member reads null until a write creates it.
  const bp = new BagPlain();
  assert(readNote(bp) === null, "absent optional member reads null");
  writeNote(bp, "gone");
  assert(readNote(bp) === "gone", "write creates an optional member across packages");

  assert(readPerimeter(new AppPerimeter()) === 8, "required accessor member across the boundary");

  // The mirror direction: the accessor lives on a class the consumer cannot name.
  const hidden = hiddenSized();
  assert(readArea(hidden) === 25, "accessor on a dependency-private class");
  assert(hidden.area === 25, "the consumer reads the same property");
  hidden.area = 6;
  assert(readArea(hidden) === 36, "the consumer writes through the setter");

  // Read-modify-write through a shaped receiver reaches both halves.
  const rmw = new ByAccessor();
  bump(rmw);
  assert(readArea(rmw) === 361, "`+=` through a shaped receiver reaches the consumer's accessor");
  inc(rmw);
  assert(readArea(rmw) === 131044, "`++` through a shaped receiver reaches it too");
  const plainRmw = new Plain();
  bump(plainRmw);
  inc(plainRmw);
  assert(readArea(plainRmw) === 12, "the same on a data-field implementation");

  onlyWrite(rmw, 5);
  assert(chainArea(rmw) === 25, "a write-only library fn reaches the consumer's setter");
  assert(chainArea(new Plain()) === 1, "a data field through a chain across the boundary");
  assert(chainArea(null) === null, "short-circuit across the boundary");

  assert(readNick(new NickAcc()) === "acc-nick", "a read-only optional member backed by a getter");
  assert(readNick(new NickNone()) === null, "a genuinely absent optional member reads null");
  assert(readNick(new NickField()) === "field-nick", "an optional data field");
  assert(chainNick(new NickAcc()) === "acc-nick", "the same through an optional chain");
  assert(chainNick(null) === null, "short-circuit on the optional member");

  assert(dump(new Plain()) === '{"area":1}', "the library serializes a data-field implementation");
  assert(
    dump(new ByAccessor()) === '{"side":3}',
    "and an accessor-backed one by its backing field",
  );
  assert(same(rmw, rmw), "identity of an accessor-backed instance across the boundary");
}
