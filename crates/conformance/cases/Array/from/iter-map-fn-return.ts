// test262: test/built-ins/Array/from/iter-map-fn-return.js
// Adapted: Symbol.iterator becomes the named iterator() protocol, and an
// exhausted result spells its `value: undefined`. `Object.is` compares objects
// structurally here, so the returned objects carry distinct contents for the
// checks to tell them apart. `nextReturnVal` and `nextNextReturnVal` are
// declared before `mapFn`, which reads them, since a closure can't read a `let`
// declared after it.

type Item = { returned?: string };

interface Items {
  iterator(): Iterator<Item>;
}

function main(): void {
  const thisVals: unknown[] = [];
  let nextResult: IteratorResult<Item> = {
    done: false,
    value: {}
  };
  let nextNextResult: IteratorResult<Item> = {
    done: false,
    value: {}
  };
  const firstReturnVal: Item = { returned: "first" };
  const secondReturnVal: Item = { returned: "second" };
  let nextReturnVal: Item | null = firstReturnVal;
  let nextNextReturnVal: Item | null = secondReturnVal;
  const mapFn = function(value: Item, idx: number): Item | null {
    const returnVal = nextReturnVal;
    nextReturnVal = nextNextReturnVal;
    nextNextReturnVal = null;
    return returnVal;
  };
  const items: Items = {
    iterator: function(): Iterator<Item> {
      return {
        next: function(): IteratorResult<Item> {
          const result = nextResult;
          nextResult = nextNextResult;
          nextNextResult = {
            done: true,
            value: undefined
          };

          return result;
        }
      };
    }
  };

  const result = Array.from(items, mapFn);

  assertSameValue(result.length, 2, 'The value of result.length is expected to be 2');
  assertSameValue(result[0], firstReturnVal, 'The value of result[0] is expected to equal the value of firstReturnVal');
  assertSameValue(
    result[1],
    secondReturnVal,
    'The value of result[1] is expected to equal the value of secondReturnVal'
  );
}
