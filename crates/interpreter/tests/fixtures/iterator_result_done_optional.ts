// SUB-1830: `IteratorYieldResult<T>` is `{ done?: false; value: T }`, as in tsc,
// so `{ value }` yields an element. Every consumer reads `done` and `value` as
// JavaScript does: getters run, and a missing or `undefined` `done` is not done.
function counter(n: number): Iterator<number> {
  let i = 0;
  const it: Iterator<number> = {
    next: (): IteratorResult<number> => {
      if (i >= n) {
        return { done: true, value: undefined };
      }
      const out: IteratorResult<number> = { value: i };
      i = i + 1;
      return out;
    },
  };
  return it;
}
function undefinedDone(n: number): Iterator<number> {
  let i = 0;
  const it: Iterator<number> = {
    next: (): IteratorResult<number> => {
      if (i >= n) {
        return { done: true, value: undefined };
      }
      const out: IteratorYieldResult<number> = { done: undefined, value: i * 10 };
      i = i + 1;
      return out;
    },
  };
  return it;
}
function source(n: number): Iterable<number> {
  const r: Iterable<number> = { iterator: (): Iterator<number> => counter(n) };
  return r;
}
function sourceUndefined(n: number): Iterable<number> {
  const r: Iterable<number> = { iterator: (): Iterator<number> => undefinedDone(n) };
  return r;
}
function pairs(): Iterable<[string, number]> {
  let i = 0;
  const it: Iterator<[string, number]> = {
    next: (): IteratorResult<[string, number]> => {
      if (i >= 2) {
        return { done: true, value: undefined };
      }
      const out: IteratorResult<[string, number]> = { value: ["k" + i, i] };
      i = i + 1;
      return out;
    },
  };
  const r: Iterable<[string, number]> = { iterator: (): Iterator<[string, number]> => it };
  return r;
}
// Results whose `done` and `value` are getters, as JavaScript reads them.
class GetterEnd {
  value: undefined = undefined;
  get done(): true {
    return true;
  }
}
class GetterYield<T> {
  constructor(readonly item: T) {}
  get done(): false {
    return false;
  }
  get value(): T {
    return this.item;
  }
}
function getters<T>(items: T[]): Iterable<T> {
  const r: Iterable<T> = {
    iterator: (): Iterator<T> => {
      let i = 0;
      const it: Iterator<T> = {
        next: (): IteratorResult<T> => {
          if (i >= items.length) {
            return new GetterEnd();
          }
          i = i + 1;
          return new GetterYield(items[i - 1]);
        },
      };
      return it;
    },
  };
  return r;
}
function undefinedDonePairs(): Iterable<[string, number]> {
  const r: Iterable<[string, number]> = {
    iterator: (): Iterator<[string, number]> => {
      let i = 0;
      const it: Iterator<[string, number]> = {
        next: (): IteratorResult<[string, number]> => {
          if (i >= 2) {
            return { done: true, value: undefined };
          }
          i = i + 1;
          const out: IteratorYieldResult<[string, number]> = { done: undefined, value: ["u" + i, i] };
          return out;
        },
      };
      return it;
    },
  };
  return r;
}
function yielded(r: IteratorResult<number>): number | undefined {
  if (!r.done) {
    const n: number = r.value;
    return n;
  }
  return undefined;
}
function main(): void {
  const r: IteratorYieldResult<number> = { value: 1 };
  assert(r.value === 1 && r.done === undefined && !("done" in r));
  assert(JSON.stringify(r) === "{\"value\":1}");
  assert(yielded({ value: 3 }) === 3 && yielded({ done: true, value: undefined }) === undefined);
  let sum = 0;
  for (const x of counter(4)) {
    sum = sum + x;
  }
  for (const x of source(4)) {
    sum = sum + x;
  }
  for (const x of sourceUndefined(3)) {
    sum = sum + x;
  }
  assert(sum === 6 + 6 + 30);
  assert(Array.from(source(3)).join(",") === "0,1,2");
  assert(Array.from(sourceUndefined(2)).join(",") === "0,10");
  assert(new Set(source(3)).size === 3 && new Set(sourceUndefined(2)).size === 2);
  assert(new Map(pairs()).get("k1") === 1);
  assert(Object.keys(Object.groupBy(source(3), (x) => (x % 2 === 0 ? "even" : "odd"))).join(",") === "even,odd");
  assert(JSON.stringify(Object.fromEntries(pairs())) === "{\"k0\":0,\"k1\":1}");
  assert(new Map(undefinedDonePairs()).get("u2") === 2);
  let viaGetters = 0;
  for (const x of getters([1, 2, 3])) {
    viaGetters = viaGetters + x;
  }
  assert(viaGetters === 6);
  assert(Array.from(getters([4, 5]), (x) => x * 2).join(",") === "8,10");
  assert(new Set(getters(["a", "b", "a"])).size === 2);
  const entries: [string, number][] = [["x", 1], ["y", 2]];
  assert(new Map(getters(entries)).get("y") === 2);
  assert(Object.keys(Object.groupBy(getters([1, 2, 3]), (x) => (x > 1 ? "big" : "small"))).join(",") === "small,big");
}
