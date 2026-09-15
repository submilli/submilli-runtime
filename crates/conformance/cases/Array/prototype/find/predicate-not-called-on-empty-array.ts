// test262: test/built-ins/Array/prototype/find/predicate-not-called-on-empty-array.js
// Adapted: undefined -> null (find returns T | null here).

function main(): void {
  let called = false;

  const predicate = (v: number): boolean => {
    called = true;
    return true;
  };

  const empty: number[] = [];
  const result = empty.find(predicate);

  assertSameValue(called, false, "[].find(predicate) does not call predicate");
  assertSameValue(result, null, "[].find(predicate) returned null");
}
