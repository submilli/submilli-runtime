// test262: test/built-ins/String/prototype/replaceAll/searchValue-flags-no-g-throws.js
// Divergence pin: JS throws a TypeError when replaceAll gets a regex without
// the g flag; here replaceAll unconditionally replaces every match (prelude
// regex doc-comment calls this out as a JS-spec divergence).

function main(): void {
  let threw = false;
  let result = "";
  try {
    result = "aaa".replaceAll(/a/, "b");
  } catch (e: Error) {
    threw = true;
  }
  assertSameValue(threw, false, "no TypeError for a non-g regex");
  assertSameValue(result, "bbb", "every match is replaced anyway");
}
