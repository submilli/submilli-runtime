// test262: test/built-ins/Array/prototype/findLast/return-found-value-predicate-result-is-true.js
// Adapted: boolean-returning predicates only (coerced returns are type errors).

function main(): void {
  const arr = ["Shoes", "Car", "Bike"];
  let called = 0;

  let result = arr.findLast((val: string): boolean => {
    called++;
    return true;
  });

  assertSameValue(result, "Bike");
  assertSameValue(called, 1, "predicate was called once");

  called = 0;
  result = arr.findLast((val: string): boolean => {
    called++;
    return val === "Shoes";
  });

  assertSameValue(called, 3, "predicate was called three times");
  assertSameValue(result, "Shoes");
}
