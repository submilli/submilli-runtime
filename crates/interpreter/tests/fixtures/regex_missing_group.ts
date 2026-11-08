// A match group reads as `string`, as in tsc's `--strict`. A group that did
// not take part in the match is a missing value (SUB-2148): a check at the
// read, or in a local the read initializes, sees `undefined`, and any other
// read throws `MissingValueError`.
function main(): void {
  const match = /(a)?b/.exec("b");
  if (match === null) throw new Error("missing match");
  const required: string = match[0];
  assert(required === "b", "the whole match is always present");

  assert((match[1] ?? "none") === "none", "`??` on a group that did not take part");
  assert(match[1] === undefined, "`=== undefined` on it");
  assert(!match[1], "`!` on it");
  assert(typeof match[1] === "undefined", "`typeof` on it");

  const [whole, first] = match;
  assert(whole === "b", "a destructured whole match");
  assert((first ?? "none") === "none", "a destructured group checked later");
  let threw = "";
  try {
    console.log(first.length);
  } catch (e: MissingValueError) {
    threw = e.message;
  }
  assert(threw === "`first` is missing: its destructured element on line 16 found no value", threw);

  let direct = "";
  try {
    console.log(match[1].length);
  } catch (e: MissingValueError) {
    direct = e.message;
  }
  assert(direct === "group 1 of `match` did not take part in the match; use `match[1] ?? …`", direct);

  const optional: string | undefined = match[1];
  assert(optional === undefined, "a place that names `undefined` takes it");

  const hit = /(a)?b/.exec("ab");
  if (hit === null) throw new Error("missing match");
  const group: string = hit[1];
  assert(group === "a", "a group that took part reads as a string");
}
