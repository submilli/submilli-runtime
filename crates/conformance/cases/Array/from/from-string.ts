// test262: test/built-ins/Array/from/from-string.js

function main(): void {
  const arrLikeSource = "Test";
  const result = Array.from(arrLikeSource);

  assertSameValue(result.length, 4, "The value of result.length is expected to be 4");
  assertSameValue(result[0], "T", 'The value of result[0] is expected to be "T"');
  assertSameValue(result[1], "e", 'The value of result[1] is expected to be "e"');
  assertSameValue(result[2], "s", 'The value of result[2] is expected to be "s"');
  assertSameValue(result[3], "t", 'The value of result[3] is expected to be "t"');
}
