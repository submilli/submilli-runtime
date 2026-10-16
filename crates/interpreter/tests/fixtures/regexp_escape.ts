// SUB-1818: RegExp.escape (ES2025) escapes every character with a meaning in
// a pattern, and a leading letter or digit, so the result matches literally.
function main(): void {
  assert(RegExp.escape("foo") === "\\x66oo");
  assert(RegExp.escape("$1.99") === "\\$1\\.99");
  assert(RegExp.escape("a-b, c=d") === "\\x61\\x2db\\x2c\\x20c\\x3dd");
  assert(RegExp.escape("(x)[y]{z}|/^*+?") === "\\(x\\)\\[y\\]\\{z\\}\\|\\/\\^\\*\\+\\?");
  assert(RegExp.escape("_\t\n\r\u000b\u000c") === "_\\t\\n\\r\\v\\f");
  assert(RegExp.escape("_  ﻿") === "_\\xa0\\u2028\\ufeff");
  assert(RegExp.escape("_é\u{1F600}") === "_é\u{1F600}");
  assert(RegExp.escape("") === "");
  const literal = "1+1=2? (yes)";
  const re = new RegExp("^" + RegExp.escape(literal) + "$");
  assert(re.test(literal) && !re.test("11=2? (yes)"));
}
