// A non-ASCII character in a regex literal matches itself — as a whole character,
// not as the bytes of its UTF-8 encoding.
function main(): void {
  assert(/é/.test("café"), "a literal non-ASCII character");
  assert(/^naïve$/.test("naïve"), "inside a longer pattern");
  assert(/[éè]/.test("père"), "inside a character class");
  assert(!/é/.test("cafe"), "still distinguishes it from ASCII");
  assert("über".replace(/ü/, "u") === "uber", "through `replace`");
  assert(/\u00e9/.test("é"), "the `\\u` escape spelling");
  assert(/\é/.test("é"), "an identity escape of a non-ASCII character");
}
