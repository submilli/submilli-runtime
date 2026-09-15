function truthy(s: string): boolean {
  return !!s;
}

function main(): void {
  assert(!truthy(""), "empty string is falsy");
  assert(truthy("a"), "non-empty string is truthy");
  assert(truthy(" "), "whitespace is truthy");
  if ("") {
    assert(false, "if (\"\") must not enter");
  }
  const x: string = "" ? "a" : "b";
  assert(x === "b", "ternary on empty string");
}
