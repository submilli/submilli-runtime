// test262: test/built-ins/String/prototype/match/S15.5.4.10_A2_T3.js
// expect-fail: match with a g-flagged RegExp should return the array of all matched substrings; it returns a single RegExpMatch (g-flag array shape not implemented)

function main(): void {
  const matches = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

  const str = "123456abcde7890";

  assertSameValue(str.match(/\d{1}/g).length, 10, "match(/\\d{1}/g).length === 10");

  for (let mi = 0; mi < matches.length; mi++) {
    assertSameValue(str.match(/\d{1}/g)[mi], matches[mi], "match(/\\d{1}/g) element " + mi.toString());
  }
}
