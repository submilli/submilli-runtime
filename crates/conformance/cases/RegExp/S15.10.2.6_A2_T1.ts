// test262: test/built-ins/RegExp/S15.10.2.6_A2_T1.js

function main(): void {
  assertSameValue(
    /^m/.test("pairs\nmakes\tdouble"),
    false,
    "^ without the m flag only matches at the start of the input",
  );
}
