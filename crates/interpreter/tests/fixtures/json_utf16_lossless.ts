function main(): void {
  const high = String.fromCharCode(0xD800);
  const low = String.fromCharCode(0xDC00);
  assert(JSON.stringify(high) === '"\\ud800"', "bare high surrogate is escaped");
  assert(JSON.stringify(low) === '"\\udc00"', "bare low surrogate is escaped");
  assert(JSON.stringify(high + low) === '"𐀀"', "valid surrogate pair stays intact");
  assert(JSON.stringify({ text: high }) === '{"text":"\\ud800"}', "typed field preserves surrogate");
  const value: unknown = { text: low };
  assert(JSON.stringify(value) === '{"text":"\\udc00"}', "unknown field preserves surrogate");
  assert(JSON.stringify({ text: high }, null, 2) === '{\n  "text": "\\ud800"\n}', "pretty output preserves surrogate");
  assert(JSON.stringify({ values: [high, low] }, null, 1) === '{\n "values": [\n  "\\ud800",\n  "\\udc00"\n ]\n}', "nested pretty output preserves surrogate");
  assert(JSON.stringify({ n: 1 }, null, "😀".repeat(6)) === '{\n' + "😀".repeat(5) + '"n": 1\n}', "indent truncates to ten UTF-16 units");
  assert(JSON.stringify({ n: 1 }, null, high.repeat(11)) === '{\n' + high.repeat(10) + '"n": 1\n}', "indent preserves lone surrogate units");
  assert(JSON.stringify({ n: 1 }, null, Number.POSITIVE_INFINITY) === '{\n          "n": 1\n}', "infinite indentation clamps to ten");
}
