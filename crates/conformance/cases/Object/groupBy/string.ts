// test262: test/built-ins/Object/groupBy/string.js
function main(): void {
  const string = "🥰💩🙏😈";
  const obj = Object.groupBy(string, (char: string): string => char < "🙏" ? "before" : "after");
  assertCompareArray(Object.keys(obj), ["after", "before"], "group keys");
  assertCompareArray(obj.before!, ["💩", "😈"], "before values");
  assertCompareArray(obj.after!, ["🥰", "🙏"], "after values");
}
