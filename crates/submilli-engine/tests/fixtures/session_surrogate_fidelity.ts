import session from "submilli:session";

function main(): void {
  // A lone high surrogate is a legal `string` code unit that UTF-8 cannot
  // carry. It must survive the store unchanged — both in a key and in a value.
  const lone = String.fromCharCode(0xd800);
  const key = "k" + lone + "z";
  const value = "v" + String.fromCharCode(0xdc00) + String.fromCharCode(0xd83d);

  session.set(key, value);
  assert(session.has(key), "a surrogate key is found by its exact code units");

  const read = session.get(key) as string;
  assert(read.length === 3, "no replacement character was substituted");
  assert(read.charCodeAt(0) === 0x76, "leading unit survives");
  assert(read.charCodeAt(1) === 0xdc00, "lone low surrogate survives");
  assert(read.charCodeAt(2) === 0xd83d, "lone high surrogate survives");

  // A key differing only in its surrogate is a different key.
  const other = "k" + String.fromCharCode(0xd801) + "z";
  assert(!session.has(other), "keys compare by exact code units");

  // A well-formed pair still round-trips as one character.
  session.set("pair", "a\u{1f600}b");
  const pair = session.get("pair") as string;
  assert(pair.length === 4, "the astral character keeps both units");
  assert(pair.charCodeAt(1) === 0xd83d, "high surrogate of the pair");
  assert(pair.charCodeAt(2) === 0xde00, "low surrogate of the pair");
}
