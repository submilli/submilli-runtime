// test262: test/built-ins/String/prototype/repeat/empty-string-returns-empty.js

function main(): void {
  assertSameValue("".repeat(1), "", '"".repeat(1)');
  assertSameValue("".repeat(3), "", '"".repeat(3)');

  const maxSafe32bitInt = 2147483647;
  assertSameValue("".repeat(maxSafe32bitInt), "", '"".repeat(maxSafe32bitInt)');
}
