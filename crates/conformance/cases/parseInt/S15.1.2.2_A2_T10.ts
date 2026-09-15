// test262: test/built-ins/parseInt/S15.1.2.2_A2_T10.js
// expect-fail: parseInt skips only ASCII whitespace; the standard's StrWhiteSpace includes the Unicode space separators (U+1680, U+2000-200A, U+202F, U+205F, U+3000), which currently yield NaN

const uspU: string[] = ["\u1680", "\u2000", "\u2001", "\u2002", "\u2003", "\u2004", "\u2005", "\u2006", "\u2007", "\u2008", "\u2009", "\u200A", "\u202F", "\u205F", "\u3000"];
const uspS: string[] = ["1680", "2000", "2001", "2002", "2003", "2004", "2005", "2006", "2007", "2008", "2009", "200A", "202F", "205F", "3000"];

function main(): void {
  for (let index = 0; index < uspU.length; index++) {
    assertSameValue(
      parseInt(uspU[index] + "1"),
      parseInt("1"),
      "parseInt(usp " + uspS[index] + " + 1) must return the same value returned by parseInt(1)",
    );

    assertSameValue(
      parseInt(uspU[index] + uspU[index] + uspU[index] + "1"),
      parseInt("1"),
      "parseInt(3x usp " + uspS[index] + " + 1) must return the same value returned by parseInt(1)",
    );

    const n: number = parseInt(uspU[index]);
    assert(n !== n, "parseInt(usp " + uspS[index] + " alone) is NaN");
  }
}
