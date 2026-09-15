// test262: test/built-ins/JSON/stringify/value-string-escape-ascii.js
// Adapted: the original builds one object whose computed property name and
// value pack every escapable ASCII character; here each character is
// asserted individually (computed property names don't exist), plus one
// string-literal property name carrying an escapable character.

function main(): void {
  const chars: string[] = [
    "\"", "\\",
    "\u0000", "\u0001", "\u0002", "\u0003",
    "\u0004", "\u0005", "\u0006", "\u0007",
    "\b", "\t", "\n", "\u000b", "\f", "\r",
    "\u000e", "\u000f", "\u0010", "\u0011",
    "\u0012", "\u0013", "\u0014", "\u0015",
    "\u0016", "\u0017", "\u0018", "\u0019",
    "\u001a", "\u001b", "\u001c", "\u001d",
    "\u001e", "\u001f",
  ];
  const jsonChars: string[] = [
    "\\\"", "\\\\",
    "\\u0000", "\\u0001", "\\u0002", "\\u0003",
    "\\u0004", "\\u0005", "\\u0006", "\\u0007",
    "\\b", "\\t", "\\n", "\\u000b", "\\f", "\\r",
    "\\u000e", "\\u000f", "\\u0010", "\\u0011",
    "\\u0012", "\\u0013", "\\u0014", "\\u0015",
    "\\u0016", "\\u0017", "\\u0018", "\\u0019",
    "\\u001a", "\\u001b", "\\u001c", "\\u001d",
    "\\u001e", "\\u001f",
  ];
  for (let i = 0; i < chars.length; i++) {
    assertSameValue(
      JSON.stringify(chars[i]),
      "\"" + jsonChars[i] + "\"",
      "ASCII 0x" + chars[i].charCodeAt(0).toString(16) + " serializes to " + jsonChars[i],
    );
    assertSameValue(
      JSON.stringify("name" + chars[i] + "value"),
      "\"name" + jsonChars[i] + "value\"",
      "embedded ASCII 0x" + chars[i].charCodeAt(0).toString(16),
    );
  }
  assertSameValue(
    JSON.stringify({ "k\n": 1 }),
    "{\"k\\n\":1}",
    "property names are escaped too",
  );
}
