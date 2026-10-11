// `Map` and `Set` iterators and `forEach` walk the collection live, as in
// JavaScript: they visit entries added after they started, skip entries
// deleted before they reach them, and follow the collection through rehashes
// and `clear`. An iterator that has finished stays finished.
function main(): void {
  const m = new Map<string, number>();
  m.set("b", 1);
  m.set("a", 2);
  const keys = m.keys();
  m.set("c", 3);
  let order = "";
  for (const key of keys) {
    order = order + key;
  }
  assert(order === "bac", "a key added before iterating is visited");

  const grow = new Map<number, number>([[0, 0]]);
  const seen: number[] = [];
  for (const [k] of grow) {
    seen.push(k);
    if (k < 40) {
      grow.set(k + 1, k);
    }
  }
  assert(seen.length === 41 && seen[40] === 40, "keys added while iterating are visited across rehashes");

  const churn = new Map<number, number>();
  for (let i = 0; i < 6; i++) {
    churn.set(i, i);
  }
  const it = churn.keys();
  it.next();
  it.next();
  churn.delete(0);
  churn.delete(3);
  for (let i = 100; i < 140; i++) {
    churn.set(i, i);
  }
  churn.delete(2);
  for (let i = 200; i < 300; i++) {
    churn.set(i, i);
  }
  churn.delete(1);
  churn.set(1, 1);
  const rest: number[] = [];
  for (const k of it) {
    rest.push(k);
  }
  assert(rest[0] === 4 && rest[1] === 5 && rest[2] === 100, "deletes and rehashes keep the position");
  assert(rest.length === 2 + 40 + 100 + 1 && rest[rest.length - 1] === 1, "a deleted key re-added comes last");

  const cleared = new Map<number, number>([[1, 1], [2, 2], [3, 3]]);
  const ci = cleared.keys();
  ci.next();
  cleared.clear();
  cleared.set(9, 9);
  const afterClear: number[] = [];
  for (const k of ci) {
    afterClear.push(k);
  }
  assert(afterClear.join(",") === "9", "after clear the iterator sees only new keys");

  const done = new Map<number, number>([[1, 1]]);
  const di = done.keys();
  di.next();
  assert(di.next().done === true, "exhausted");
  done.set(2, 2);
  assert(di.next().done === true, "an exhausted iterator stays done");

  const fe = new Map<number, number>([[1, 1]]);
  const feSeen: number[] = [];
  fe.forEach((v: number, k: number) => {
    feSeen.push(k);
    if (k < 5) {
      fe.set(k + 1, v);
    }
    if (k === 3) {
      fe.clear();
      fe.set(10, 0);
    }
  });
  assert(feSeen.join(",") === "1,2,3,10", "Map#forEach visits added keys and follows clear");

  const s = new Set<number>([1, 2]);
  const sv = s.values();
  sv.next();
  s.add(3);
  s.delete(2);
  for (let i = 10; i < 30; i++) {
    s.add(i);
  }
  const sRest: number[] = [];
  for (const v of sv) {
    sRest.push(v);
  }
  assert(sRest[0] === 3 && sRest.length === 21, "Set iterators see added elements");

  const sf = new Set<number>([1]);
  const sfSeen: number[] = [];
  sf.forEach((v: number) => {
    sfSeen.push(v);
    if (v < 20) {
      sf.add(v + 1);
    }
  });
  assert(sfSeen.length === 20, "Set#forEach visits added elements");

  const se = new Set<number>([1, 2]);
  const sei = se.entries();
  se.clear();
  assert(sei.next().done === true, "a cleared set's iterator is done");
}
