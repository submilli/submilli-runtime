// String#replace and String#split with a RegExp follow JavaScript: the
// replacement pattern is GetSubstitution, a split includes captures, and lone
// surrogates outside and inside matches survive.
function hex(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) {
    out.push(s.charCodeAt(i).toString(16));
  }
  return out.join(" ");
}

function main(): void {
  assert("abc".replace(/b/, "[$&]") === "a[b]c", "$& is the match");
  assert("abc".replace(/(b)/, "[$1x]") === "a[bx]c", "$1 then a literal");
  assert("abc".replace(/(?<n>b)/, "[$<n>]") === "a[b]c", "$<name>");
  assert("abc".replace(/(?<n>b)/, "[$<m>]") === "a[]c", "an unknown name is empty");
  assert("abc".replace(/(?<n>b)/, "[$<n]") === "a[$<n]c", "an unclosed name is literal");
  assert("abc".replace(/(b)/, "[$<n>]") === "a[$<n>]c", "$< without named groups is literal");
  assert("abc".replace(/(?<n>b)/, "[${n}]") === "a[${n}]c", "${n} is literal");
  assert("abc".replace(/b/, "[$`|$']") === "a[a|c]c", "prefix and suffix");
  assert("abc".replace(/(b)/, "[$2|$01|$10|$0]") === "a[$2|b|b0|$0]c", "group numbers");
  assert("abc".replace(/b/, "$$|$") === "a$|$c", "dollars");
  assert("abc".replace(/(x)?b/, "[$1]") === "a[]c", "an unmatched group is empty");
  assert("abc".replace(/b*/g, "-") === "-a--c-", "empty matches advance");
  assert("aaa".replaceAll(/a*?/g, "-") === "-a-a-a-", "lazy empty matches");

  assert("a,b".split(/(,)/).join("|") === "a|,|b", "split keeps captures");
  assert("abc".split(/(?:)/).join("|") === "a|b|c", "split on empty matches");
  assert("abc".split(/b*/).join("|") === "a|c", "an empty match at the part start");
  assert("".split(/x/).length === 1, "an empty input without a match");
  assert("".split(/(?:)/).length === 0, "an empty input with a match");
  assert("a1b2c".split(/(\d)/, 2).join("|") === "a|1", "the limit cuts captures");
  const parts = "ab".split(/(x)?b/);
  assert(parts.length === 3 && parts[0] === "a" && parts[2] === "", "an unmatched capture");

  const lone = "a" + String.fromCharCode(0xd800) + "b";
  assert(hex(lone.replace(/b/g, "X")) === "61 d800 58", "replace keeps the prefix");
  assert(hex(lone.replace(/a/, "$'")) === "d800 62 d800 62", "$' keeps the suffix");
  assert(hex(lone.split(/b/)[0]) === "61 d800", "split keeps the part");
}
