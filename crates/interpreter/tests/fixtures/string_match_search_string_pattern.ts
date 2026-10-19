// SUB-1820: match and search take a string, compiled as a pattern the way
// JavaScript's `new RegExp(s)` does, as tsc's lib allows.
function main(): void {
  const text = "order 42, item 7";
  const found = text.match("\\d+");
  assert(found !== null && found.index === 6);
  assert(text.match("x+") === null);
  assert(text.search("item") === 10 && text.search("a.b") === -1);
  assert("a.b".search(".") === 0 && "xa.b".search("\\.") === 2);
  const viaRegex = text.match(/\d+$/);
  assert(viaRegex !== null && viaRegex.index === 15);
  let invalid = false;
  try {
    text.search("(");
  } catch (e) {
    invalid = e instanceof SyntaxError;
  }
  assert(invalid);
}
