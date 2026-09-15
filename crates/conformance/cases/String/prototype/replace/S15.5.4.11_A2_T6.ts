// test262: test/built-ins/String/prototype/replace/S15.5.4.11_A2_T6.js

function main(): void {
  const str = "She sells seashells by the seashore.";
  const re = /sh/;

  assertSameValue(
    str.replace(re, "sch"),
    "She sells seaschells by the seashore.",
    "non-g replace rewrites only the first match",
  );
}
