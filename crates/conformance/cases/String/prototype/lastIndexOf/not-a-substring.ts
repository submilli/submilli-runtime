// test262: test/built-ins/String/prototype/lastIndexOf/not-a-substring.js

function main(): void {
  assertSameValue(
    "abc".lastIndexOf("d"),
    -1,
    "String.prototype.lastIndexOf returns -1 when searchString is shorter than this and searchString is not a substring of this.",
  );
}
