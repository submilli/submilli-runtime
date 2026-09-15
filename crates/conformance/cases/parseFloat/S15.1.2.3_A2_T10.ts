// test262: test/built-ins/parseFloat/S15.1.2.3_A2_T10.js
// expect-fail: parseFloat skips only ASCII whitespace; the standard's StrWhiteSpace includes the Unicode space separators (U+1680, U+2000-200A, U+202F, U+205F, U+3000), which currently yield NaN

const uspU: string[] = ["\u1680", "\u2000", "\u2001", "\u2002", "\u2003", "\u2004", "\u2005", "\u2006", "\u2007", "\u2008", "\u2009", "\u200A", "\u202F", "\u205F", "\u3000"];
const uspS: string[] = ["1680", "2000", "2001", "2002", "2003", "2004", "2005", "2006", "2007", "2008", "2009", "200A", "202F", "205F", "3000"];

function main(): void {
  for (let index = 0; index < uspU.length; index++) {
    assertSameValue(
      parseFloat(uspU[index] + "1.1"),
      parseFloat("1.1"),
      "parseFloat(usp " + uspS[index] + " + 1.1) must equal parseFloat(1.1)",
    );

    assertSameValue(
      parseFloat(uspU[index] + uspU[index] + uspU[index] + "1.1"),
      parseFloat("1.1"),
      "parseFloat(3x usp " + uspS[index] + " + 1.1) must equal parseFloat(1.1)",
    );

    const n: number = parseFloat(uspU[index]);
    assert(n !== n, "parseFloat(usp " + uspS[index] + " alone) is NaN");
  }
}
