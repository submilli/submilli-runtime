// SUB-1819: replace and replaceAll take a replacer function, as in tsc's lib.
// It gets the match, each capture (undefined if it didn't take part), the
// offset, the whole string and, for named groups, an object of them.
function main(): void {
  assert("a-b-c".replace(/-/g, (m) => m + m) === "a--b--c");
  assert("a-b-c".replace("-", (m) => "[" + m + "]") === "a[-]b-c");
  assert("a-b-c".replaceAll("-", () => "+") === "a+b+c");
  assert("x1y22".replace(/\d+/g, (m) => String(Number(m) * 2)) === "x2y44");
  const swapped = "john smith".replace(/(\w+) (\w+)/, (_m, first, last) => String(last) + " " + String(first));
  assert(swapped === "smith john");
  const offsets: number[] = [];
  "abab".replaceAll("b", (m, offset) => {
    offsets.push(offset as number);
    return m.toUpperCase();
  });
  assert(offsets.join(",") === "1,3");
  const seen: string[] = [];
  "ac".replace(/a(b)?c/, (m, group, offset, whole) => {
    seen.push(m, String(group), String(offset), String(whole));
    return "";
  });
  assert(seen.join("|") === "ac|undefined|0|ac");
  const named = "2026-10".replace(/(?<year>\d+)-(?<month>\d+)/, (...args: unknown[]) => {
    return String(args.length);
  });
  assert(named === "6");
  assert("aaa".replace(/a/, (m) => m.toUpperCase()) === "Aaa");
  assert("ab".replace(/x*/g, () => "-") === "-a-b-");
  assert("€x€".replaceAll("€", (m, i) => String(i)) === "0x2");
}
