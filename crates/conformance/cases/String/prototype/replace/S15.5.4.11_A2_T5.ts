// test262: test/built-ins/String/prototype/replace/S15.5.4.11_A2_T5.js
// expect-fail: the $' replacement token (substring after the match) is not substituted — it passes through literally ($$ and $N do substitute)

function main(): void {
  const str = "She sells seashells by the seashore.";
  const re = /sh/g;

  assertSameValue(
    str.replace(re, "$'" + "sch"),
    "She sells seaells by the seashore.schells by the seaore.schore.",
    "$' substitutes the substring after the match",
  );
}
