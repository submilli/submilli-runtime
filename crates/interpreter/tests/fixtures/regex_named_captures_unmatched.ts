// An alternation where only one named group participates: the other named
// capture is present in the pattern but unmatched. `namedGroups` preserves the
// unmatched name with an undefined value.
function main(): void {
  const re: RegExp = new RegExp("(?<a>x)|(?<b>y)", "");

  const m: RegExpExecArray | null = re.exec("y");
  assert(m !== null, "matched the second alternative");
  if (m !== null) {
    assert(m[0] === "y", "matched text is y");
    const groups = m.groups;
    if (groups === undefined) { throw new Error("missing named groups"); }
    assert(Object.keys(groups).includes("b"), "matched named capture b is present");
    assert(groups["b"] === "y", "b captured y");
    assert(Object.keys(groups).includes("a"), "unmatched named capture a is present");
    assert(groups["a"] === undefined, "unmatched named capture a is undefined");
    assert(Object.keys(groups).length === 2, "both names are in the map");
  }

  const m2: RegExpExecArray | null = re.exec("x");
  assert(m2 !== null, "matched the first alternative");
  if (m2 !== null) {
    const groups2 = m2.groups;
    if (groups2 === undefined) { throw new Error("missing named groups"); }
    assert(Object.keys(groups2).includes("a"), "matched named capture a is present");
    assert(groups2["a"] === "x", "a captured x");
    assert(Object.keys(groups2).includes("b"), "unmatched named capture b is present");
    assert(groups2["b"] === undefined, "unmatched named capture b is undefined");
  }
}
