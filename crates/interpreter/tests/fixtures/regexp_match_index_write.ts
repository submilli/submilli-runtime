// `RegExpMatchArray` extends `Array<string>`, so an element can be written,
// as on any array.
function main(): void {
  const m = "ab".match(/(a)(x)?(b)/)!;
  m[1] = "z";
  m[2] = "y";
  m[0] += "!";
  assert(m[1] === "z" && m[2] === "y" && m[0] === "ab!", "elements are replaced");
  assert(m.length === 4, "the length is unchanged");
  assert(m.join(",") === "ab!,z,y,b", "array methods see the writes");

  const e: RegExpExecArray = /(c)/.exec("c")!;
  e[1] = e[1] + "2";
  assert(e[1] === "c2", "an exec result is written the same way");

  let threw = false;
  try {
    m[9] = "q";
  } catch (err) {
    threw = err instanceof RangeError;
  }
  assert(threw, "a write past the end follows the array rules");
}
