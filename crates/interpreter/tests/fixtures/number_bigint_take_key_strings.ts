// SUB-2106: a key string from `Object.keys` or `Object.entries` is a string
// everywhere: `Number()`, `BigInt()` and the string-or-RegExp methods accept it.
function showArtist(id: number): number {
  return id;
}
function main(): void {
  const artistIds: Record<string, string> = {};
  artistIds[String(12)] = "a";
  artistIds[String(34)] = "b";
  let total = 0;
  for (const id of Object.keys(artistIds)) {
    total = total + showArtist(Number(id));
  }
  assert(total === 46);

  const optional: { "7"?: number } = { "7": 1 };
  const key = Object.keys(optional)[0];
  const [entryKey] = Object.entries(artistIds)[0];
  const parsed = JSON.parse("{\"56\": 1}") as Record<string, number>;
  const parsedKey = Object.keys(parsed)[0];
  for (const k of [key, entryKey, parsedKey]) {
    const n = Number(k);
    assert(Number(String(k)) === n && Number(`${k}`) === n && n > 0);
    assert(BigInt(k) === BigInt(n));
  }
  assert("a7b".replace(key, "-") === "a-b" && "a7b7".replaceAll(key, "") === "ab");
  assert("a7b".split(key).join("|") === "a|b" && "a7b".search(key) === 1);
  const found = "a7b".match(key);
  assert(found !== null && found[0] === "7");
}
