// test262: test/built-ins/RegExp/nullable-quantifier.js
// expect-fail: ECMA-262 RepeatMatcher lets /(a?b??)*/ on "ab" match "ab" in two iterations (the b?? arm may match on a later iteration); the engine stops after "a"
// RegExpExecArray shape adapted to RegExpMatch.

function main(): void {
  const m = /(a?b??)*/.exec("ab");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  assertSameValue(matched, "ab", "the regex is expected to match the whole string");
}
