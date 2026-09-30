// A host function that re-enters the program holds values in its own memory:
// the results its callbacks return, and the elements it read before a callback
// dropped them from their container. Each callback here allocates enough to
// make the collector run, so a value the host failed to keep reachable comes
// back as a different object, or as none.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

interface Source<T> {
  iterator(): Iterator<T>;
}

class Many {
  get items(): Box[] {
    return boxes(120);
  }
}

class Lazy {
  get first(): Box[] {
    churn();
    return [new Box(1), new Box(2)];
  }
  get second(): Box[] {
    churn();
    return [new Box(3), new Box(4)];
  }
}

// More than the heap grows by in one step, so the collector runs; the small
// objects then take the slots it freed.
function churn(): void {
  const large = "x".repeat(40000);
  for (let i = 0; i < 16; i++) {
    const small = [large.length.toString(), "more"];
  }
}

function total(boxes: Box[]): number {
  let sum = 0;
  for (const box of boxes) {
    sum += box.v;
  }
  return sum;
}

function boxes(count: number): Box[] {
  const made: Box[] = [];
  for (let i = 1; i <= count; i++) {
    made.push(new Box(i));
  }
  return made;
}

function source(count: number): Source<Box> {
  return {
    iterator: (): Iterator<Box> => {
      let n = 0;
      const it: Iterator<Box> = {
        next: (): IteratorResult<Box> => {
          churn();
          n++;
          if (n > count) {
            const done: IteratorResult<Box> = { done: true };
            return done;
          }
          const step: IteratorResult<Box> = { done: false, value: new Box(n) };
          return step;
        },
      };
      return it;
    },
  };
}

function pairSource(count: number): Source<[Box, Box]> {
  return {
    iterator: (): Iterator<[Box, Box]> => {
      let n = 0;
      const it: Iterator<[Box, Box]> = {
        next: (): IteratorResult<[Box, Box]> => {
          churn();
          n++;
          if (n > count) {
            const done: IteratorResult<[Box, Box]> = { done: true };
            return done;
          }
          const step: IteratorResult<[Box, Box]> = {
            done: false,
            value: [new Box(n), new Box(n * 10)],
          };
          return step;
        },
      };
      return it;
    },
  };
}

function callbackResults(): void {
  const mapped = [1, 2, 3, 4, 5, 6, 7, 8].map((x: number) => {
    churn();
    return new Box(x);
  });
  churn();
  assert(total(mapped) === 36, "map keeps each callback's result");

  const flat = [1, 2, 3, 4, 5, 6, 7, 8].flatMap((x: number) => {
    churn();
    return [new Box(x), new Box(x * 10)];
  });
  churn();
  assert(total(flat) === 396, "flatMap keeps each returned array's elements");

  const reused: Box[] = [new Box(0)];
  const fromReused = [1, 2, 3, 4].flatMap((x: number) => {
    churn();
    reused[0] = new Box(x);
    return reused;
  });
  churn();
  assert(total(fromReused) === 10, "flatMap keeps elements of an array the callback reuses");

  // Reading the index makes the host box it, an allocation made while only
  // the host holds the accumulator.
  const folded = [1, 2, 3, 4, 5, 6, 7, 8].reduce((acc: Box, x: number, i: number) => {
    churn();
    return new Box(acc.v + x + i);
  }, new Box(0));
  assert(folded.v === 64, "reduce keeps the accumulator");

  // Each byte is boxed for the callback while only the host holds the
  // accumulator, so enough bytes make the collector run on their own.
  const bytes = new Uint8Array(2000);
  bytes.fill(1);
  const foldedBytes = bytes.reduce((acc: Box, x: number) => new Box(acc.v + x), new Box(0));
  assert(foldedBytes.v === 2000, "Uint8Array reduce keeps the accumulator");
}

function arrayFrom(): void {
  const fromArray = Array.from([1, 2, 3, 4, 5, 6, 7, 8], (x: number) => {
    churn();
    return new Box(x);
  });
  churn();
  assert(total(fromArray) === 36, "Array.from keeps mapFn results over an array");

  const fromString = Array.from("abcdefgh", (unit: string) => {
    churn();
    return new Box(unit.length);
  });
  churn();
  assert(total(fromString) === 8, "Array.from keeps mapFn results over a string");

  const fromIterator = Array.from(source(8));
  churn();
  assert(fromIterator.length === 8, "Array.from drains an iterator");
  assert(total(fromIterator) === 36, "Array.from keeps what an iterator yields");

  const fromBoth = Array.from(source(8), (box: Box) => {
    churn();
    return new Box(box.v * 2);
  });
  churn();
  assert(total(fromBoth) === 72, "Array.from keeps mapFn results over an iterator");
}

function droppedElements(): void {
  let seen = 0;
  const walked = boxes(8);
  walked.forEach((box: Box) => {
    walked.splice(0, walked.length);
    churn();
    seen += box.v;
  });
  assert(seen === 36, "forEach keeps elements its callback removed");

  const filtered = boxes(8);
  const kept = filtered.filter((box: Box) => {
    filtered.splice(0, filtered.length);
    churn();
    return box.v > 0;
  });
  churn();
  assert(total(kept) === 36, "filter keeps elements its callback removed");

  const searched = boxes(8);
  const found = searched.find((box: Box) => {
    searched.splice(0, searched.length);
    churn();
    return box.v === 8;
  });
  churn();
  assert(found !== null && found.v === 8, "find keeps elements its callback removed");

  const tested = boxes(8);
  const every = tested.every((box: Box) => {
    tested.splice(0, tested.length);
    churn();
    return box.v > 0;
  });
  assert(every, "every keeps elements its callback removed");

  const sorted: Box[] = [];
  for (let i = 0; i < 8; i++) {
    sorted.push(new Box(8 - i));
  }
  sorted.sort((a: Box, b: Box) => {
    sorted.splice(0, sorted.length);
    churn();
    return a.v - b.v;
  });
  churn();
  assert(sorted.length === 8, "sort writes its elements back");
  assert(total(sorted) === 36, "sort keeps elements its comparator removed");
  assert(sorted[0].v === 1 && sorted[7].v === 8, "sort orders the kept elements");

  for (let round = 0; round < 6; round++) {
    const spliced = boxes(1500);
    const removed = spliced.splice(0, 1499);
    assert(total(removed) + total(spliced) === 1125750, "splice returns the elements it removed");
  }
}

function collections(): void {
  const map = new Map<Box, Box>(pairSource(8));
  churn();
  let mapSum = 0;
  map.forEach((value: Box, key: Box) => {
    mapSum += value.v + key.v;
  });
  assert(map.size === 8, "Map keeps each entry an iterator yields");
  assert(mapSum === 396, "Map keeps the keys and values an iterator yields");

  const set = new Set<Box>(source(8));
  churn();
  let setSum = 0;
  set.forEach((value: Box) => {
    setSum += value.v;
  });
  assert(set.size === 8, "Set keeps each element an iterator yields");
  assert(setSum === 36, "Set keeps the elements an iterator yields");

  const cleared = new Map<number, Box>();
  for (let i = 0; i < 8; i++) {
    cleared.set(i, new Box(i + 1));
  }
  let clearedSum = 0;
  cleared.forEach((value: Box) => {
    cleared.clear();
    churn();
    clearedSum += value.v;
  });
  assert(clearedSum === 36, "Map forEach keeps the entries its callback cleared");

  const emptied = new Set<Box>();
  for (let i = 0; i < 8; i++) {
    emptied.add(new Box(i + 1));
  }
  let emptiedSum = 0;
  emptied.forEach((value: Box) => {
    emptied.clear();
    churn();
    emptiedSum += value.v;
  });
  assert(emptiedSum === 36, "Set forEach keeps the elements its callback cleared");
}

function getters(): void {
  const json = JSON.stringify(new Lazy());
  assert(
    json === '{"first":[{"v":1},{"v":2}],"second":[{"v":3},{"v":4}]}',
    "JSON.stringify keeps what a getter returns",
  );
  const many = JSON.stringify(new Many());
  assert(
    many.startsWith('{"items":[{"v":1},{"v":2},') && many.endsWith(',{"v":119},{"v":120}]}'),
    "JSON.stringify keeps a getter's array while it serializes the elements",
  );
}

function main(): void {
  callbackResults();
  arrayFrom();
  droppedElements();
  collections();
  getters();
}
