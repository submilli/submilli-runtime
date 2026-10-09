// test262: test/built-ins/Array/prototype/includes/search-not-found-returns-false.js
// Adapted: mixed-type rows use union element types; Symbol dropped. The
// {}/[] rows assert JS reference identity, which our structural ===
// replaces — dropped (README caveat).

function main(): void {
  assertSameValue([42].includes(43), false, "43");

  assertSameValue(["test262"].includes("test"), false, "string");

  const mixed: (number | string | undefined)[] = [0, "test262", undefined];
  assertSameValue(mixed.includes(""), false, "the empty string");

  const stringOrFalse: (string | boolean)[] = ["true", false];
  assertSameValue(stringOrFalse.includes(true), false, "true");
  const stringOrTrue: (string | boolean)[] = ["", true];
  assertSameValue(stringOrTrue.includes(false), false, "false");

  const noNull: (number | boolean | undefined | null)[] = [undefined, false, 0, 1];
  assertSameValue(noNull.includes(null), false, "null");
  const noUndefined: (null | undefined)[] = [null];
  assertSameValue(noUndefined.includes(undefined), false, "undefined");
}
