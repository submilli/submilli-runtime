function main(): void {
  assert("abc".at(3) === undefined, "string at miss");
  assert("abc".at(-1) === "c", "string at negative index");
  assert("abc".codePointAt(3) === undefined, "code point miss");
  assert("😀".codePointAt(0) === 128512, "surrogate pair code point");
  assert(String.fromCharCode(55296).codePointAt(0) === 55296, "lone surrogate code point");
  assert("abc".charAt(3) === "", "charAt keeps empty string");
  assert(Number.isNaN("abc".charCodeAt(3)), "charCodeAt keeps NaN");
  assert("abc".slice(1, undefined) === "bc", "explicit undefined numeric default");
  assert("a".padStart(2, undefined) === " a", "explicit undefined string default");
  const match = /(?<first>a)|(?<second>b)/.exec("a");
  assert(match !== null, "regex matched");
  if (match !== null) {
    assert(match.groups[0] === "a", "participating capture");
    assert(match.groups[1] === undefined, "unmatched capture");
    assert(match.namedGroups.has("second"), "unmatched named capture is present");
    assert(match.namedGroups.get("second") === undefined, "unmatched named capture value");
    assert(match.namedGroups.get("first") === "a", "participating named capture");
  }
  assert(/z/.exec("a") === null, "regex failed match keeps null");
  assert("a".match(/z/) === null, "string failed match keeps null");
}
