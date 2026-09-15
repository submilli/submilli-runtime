// test262: test/built-ins/Array/prototype/find/predicate-call-parameters.js
// expect-fail: standard predicates receive (value, index, array); our find callback type is (T) => boolean, so a three-parameter predicate is a compile error

function main(): void {
  const arr = ["Mike", "Rick", "Leo"];

  let calls = 0;
  let sawIndexes = true;
  let sawArray = true;

  arr.find((v: string, i: number, a: string[]): boolean => {
    if (i !== calls) {
      sawIndexes = false;
    }
    if (a.length !== arr.length) {
      sawArray = false;
    }
    calls++;
    return false;
  });

  assertSameValue(calls, 3, "predicate called once per element");
  assert(sawIndexes, "predicate receives the index as second argument");
  assert(sawArray, "predicate receives the array as third argument");
}
