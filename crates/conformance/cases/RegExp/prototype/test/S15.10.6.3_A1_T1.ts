// test262: test/built-ins/RegExp/prototype/test/S15.10.6.3_A1_T1.js

function main(): void {
  const str = "123";
  assertSameValue(
    /1|12/.test(str),
    /1|12/.exec(str) !== null,
    "test(s) agrees with exec(s) !== null",
  );
}
