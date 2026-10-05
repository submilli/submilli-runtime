// `decodeURI` and `decodeURIComponent` copy every code unit outside a `%`
// escape as it is, a lone surrogate included, as JavaScript does. An escape
// that encodes a surrogate is still malformed.

function units(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) {
    out.push(s.charCodeAt(i).toString(16));
  }
  return out.join(" ");
}

function decoded(f: (s: string) => string, s: string): string {
  try {
    return units(f(s));
  } catch (e) {
    return "throws";
  }
}

function show(label: string, actual: string, expected: string): void {
  console.log(label, actual);
  assert(actual === expected, label);
}

function main(): void {
  const high = String.fromCharCode(0xd800);
  const low = String.fromCharCode(0xdfff);
  show("high", decoded(decodeURIComponent, high), "d800");
  show("low", decoded(decodeURI, low), "dfff");
  show("same string", String(decodeURIComponent(high) === high), "true");
  show("pair", decoded(decodeURIComponent, high + String.fromCharCode(0xdc00)), "d800 dc00");
  show(
    "between escapes",
    decoded(decodeURIComponent, "a" + high + "%41" + low + "%E2%82%AC"),
    "61 d800 41 dfff 20ac",
  );
  show("reserved kept", decoded(decodeURI, low + high + "%23"), "dfff d800 25 32 33");
  show("reserved case kept", decoded(decodeURI, "%2f" + high), "25 32 66 d800");
  show("astral escape", decoded(decodeURIComponent, "%F0%9F%98%80" + high), "d83d de00 d800");
  show("escaped surrogate", decoded(decodeURIComponent, "%ED%A0%80"), "throws");
  show("short escape", decoded(decodeURIComponent, high + "%E2%82"), "throws");
  show("surrogate in escape", decoded(decodeURIComponent, "%" + high + "1"), "throws");
}
