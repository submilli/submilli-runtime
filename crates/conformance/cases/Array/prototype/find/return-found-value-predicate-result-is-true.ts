// test262: test/built-ins/Array/prototype/find/return-found-value-predicate-result-is-true.js
// Adapted: predicates return boolean (the coerced string/object/Symbol/number
// return variants are compile-time type errors here).

function main(): void {
  const arr = ["Shoes", "Car", "Bike"];
  let called = 0;

  let result = arr.find((val: string): boolean => {
    called++;
    return true;
  });

  assertSameValue(result, "Shoes");
  assertSameValue(called, 1, "predicate was called once");

  called = 0;
  result = arr.find((val: string): boolean => {
    called++;
    return val === "Bike";
  });

  assertSameValue(called, 3, "predicate was called three times");
  assertSameValue(result, "Bike");
}
