// Unicode space separators are whitespace, as in JavaScript. The next three lines
// hold, in order, U+00A0 (no-break space) before `const`, U+3000 (ideographic space)
// after `const`, and U+202F (narrow no-break space) before `=`.
function main(): void {
  const a = 1;
  const　b = 2;
  const c = a + b;
  assert(c === 3, "separators between tokens");
  assert(`x y`.charCodeAt(1) === 160, "inside a template it is text, not whitespace");
  assert("a\/b" === "a/b", "`\\/` is an escaped slash");
}
