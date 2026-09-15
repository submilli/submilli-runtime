// test262: test/built-ins/RegExp/S15.10.2.10_A5.1_T1.js
// expect-fail: \< and \> should be identity escapes per ECMA-262; the engine parses them as start/end word-boundary assertions, so they never match the literal character (exec returns null here — the input has no word characters)

function main(): void {
  const nonIdent = "~`!@#$%^&*()-+={[}]|\\:;'<,>./?" + "\"";
  for (let k = 0; k < nonIdent.length; k++) {
    const c = nonIdent.charAt(k);
    const arr = new RegExp("\\" + c, "g").exec(nonIdent);
    assert(arr !== null, "no match for character: " + c);
    if (arr === null) {
      return;
    }
    const matched = arr.match;
    assertSameValue(matched, c, "identity escape matches the literal character " + c);
  }
}
