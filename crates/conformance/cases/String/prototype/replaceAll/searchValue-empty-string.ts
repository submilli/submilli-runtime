// test262: test/built-ins/String/prototype/replaceAll/searchValue-empty-string.js

function main(): void {
  let result = "aab c  \nx".replaceAll("", "_");
  assertSameValue(result, "_a_a_b_ _c_ _ _\n_x_");

  result = "a".replaceAll("", "_");
  assertSameValue(result, "_a_");
}
