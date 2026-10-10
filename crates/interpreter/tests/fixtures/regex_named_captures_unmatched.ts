// An alternation where only one named group participates: the other named
// capture is present in the pattern but unmatched. `namedGroups` preserves the
// unmatched name with an undefined value.
function main(): void {
  const re: RegExp = new RegExp("(?<a>x)|(?<b>y)", "");

  const m: RegExpMatch | null = re.exec("y");
  assert(m !== null, "matched the second alternative");
  if (m !== null) {
    assert(m.match === "y", "matched text is y");
    const groups: Map<string, string | undefined> = m.namedGroups;
    assert(groups.has("b"), "matched named capture b is present");
    assert(groups.get("b") === "y", "b captured y");
    assert(groups.has("a"), "unmatched named capture a is present");
    assert(groups.get("a") === undefined, "unmatched named capture a is undefined");
    assert(groups.size === 2, "both names are in the map");
  }

  const m2: RegExpMatch | null = re.exec("x");
  assert(m2 !== null, "matched the first alternative");
  if (m2 !== null) {
    const groups2: Map<string, string | undefined> = m2.namedGroups;
    assert(groups2.has("a"), "matched named capture a is present");
    assert(groups2.get("a") === "x", "a captured x");
    assert(groups2.has("b"), "unmatched named capture b is present");
    assert(groups2.get("b") === undefined, "unmatched named capture b is undefined");
  }
}
