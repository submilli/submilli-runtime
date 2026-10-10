// test262: test/built-ins/Array/from/iter-map-fn-return.js
// Adapted: Symbol.iterator becomes the named iterator() protocol; the mapper
// takes only the value (no index); reference-identity assertions on the
// returned objects become value checks.

interface TwoNumbers {
  iterator(): Iterator<number>;
}

function twoNumbers(): TwoNumbers {
  return {
    iterator: (): Iterator<number> => {
      let i = 0;
      const it: Iterator<number> = {
        next: (): IteratorResult<number> => {
          if (i >= 2) {
            const done: IteratorResult<number> = { done: true, value: undefined };
            return done;
          }
          i = i + 1;
          const step: IteratorResult<number> = { done: false, value: i };
          return step;
        },
      };
      return it;
    },
  };
}

function main(): void {
  const mapFn = (value: number): number => value * 10;

  const result = Array.from(twoNumbers(), mapFn);

  assertSameValue(result.length, 2, "The value of result.length is expected to be 2");
  assertSameValue(result[0], 10, "result[0] is the mapper's first return value");
  assertSameValue(result[1], 20, "result[1] is the mapper's second return value");
}
