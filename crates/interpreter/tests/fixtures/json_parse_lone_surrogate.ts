// `JSON.parse` keeps a lone surrogate, escaped or raw, as JavaScript strings
// carry it, and keeps the sign of `-0`.
function main(): void {
  const escaped = JSON.parse('"\\ud800x"') as string;
  assert(escaped.length === 2 && escaped.charCodeAt(0) === 0xd800, "an escaped lone surrogate");

  const raw = JSON.parse(`"${String.fromCharCode(0xdc00)}"`) as string;
  assert(raw.length === 1 && raw.charCodeAt(0) === 0xdc00, "a raw lone surrogate");

  const pair = JSON.parse('"\\ud83d\\ude00"') as string;
  assert(pair === String.fromCharCode(0xd83d, 0xde00), "an escaped pair is one code point");

  const keyed = JSON.parse('{"\\udfff":1}') as Record<string, number>;
  assert(keyed[String.fromCharCode(0xdfff)] === 1, "a lone surrogate in a key");

  assert(Object.is(JSON.parse("-0") as number, -0), "-0 keeps its sign");
  assert(JSON.stringify(JSON.parse('[1,"a",true,null,{"b":2}]')) === '[1,"a",true,null,{"b":2}]', "a round trip");

  let refused = "";
  for (const text of ["{", "[1,]", "01", '"\\x"', "1 2"]) {
    try {
      JSON.parse(text);
    } catch (e) {
      if (e instanceof SyntaxError) refused += "s";
    }
  }
  assert(refused === "sssss", "malformed text throws SyntaxError");
  console.log(escaped.length, raw.charCodeAt(0).toString(16), refused);
}
