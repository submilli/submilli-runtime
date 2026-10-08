// test262: test/built-ins/String/prototype/replace/S15.5.4.11_A2_T4.js

function main(): void {
  const str = "She sells seashells by the seashore.";
  const re = /sh/g;

  assertSameValue(
    str.replace(re, "$`" + "sch"),
    "She sells seaShe sells seaschells by the seaShe sells seashells by the seaschore.",
    "$` substitutes the substring before the match",
  );
}
