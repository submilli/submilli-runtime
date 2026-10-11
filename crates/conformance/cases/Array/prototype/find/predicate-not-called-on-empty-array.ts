// test262: test/built-ins/Array/prototype/find/predicate-not-called-on-empty-array.js

function main(): void {
  let called = false;

  const predicate = (v: number): boolean => {
    called = true;
    return true;
  };

  const empty: number[] = [];
  const result = empty.find(predicate);

  assertSameValue(called, false, "[].find(predicate) does not call predicate");
  assertSameValue(result, undefined, "[].find(predicate) returned undefined");
}
