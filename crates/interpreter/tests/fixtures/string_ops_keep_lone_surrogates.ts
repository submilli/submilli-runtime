// String operations keep a lone surrogate as the code unit it is, as
// JavaScript does, rather than turning it into U+FFFD.
function hex(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) {
    out.push(s.charCodeAt(i).toString(16));
  }
  return out.join(" ");
}

function main(): void {
  const lone = "a" + String.fromCharCode(0xd800) + "b";
  assert(hex(lone.toUpperCase()) === "41 d800 42", "toUpperCase");
  assert(hex((" " + lone + " ").trim()) === "61 d800 62", "trim");
  assert(JSON.stringify(lone) === "\"a\\ud800b\"", "JSON.stringify escapes it");

  const marks = "é" + String.fromCharCode(0xd800) + "́aé";
  assert(hex(marks.normalize("NFC")) === "e9 d800 301 61 e9", "NFC");
  assert(hex(marks.normalize("NFD")) === "65 301 d800 301 61 65 301", "NFD");
}
