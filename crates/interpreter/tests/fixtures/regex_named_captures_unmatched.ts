// An alternation where only one named group participates: the other named
// capture is present in the pattern but unmatched. `namedGroups` must omit the
// unmatched name rather than trap (the Wasm body's deferred null-skip, resolved
// in the Rust host port).
function main(): void {
  const re: RegExp = new RegExp("(?<a>x)|(?<b>y)", "");

  const m: RegExpMatch | null = re.exec("y");
  assert(m !== null, "matched the second alternative");
  if (m !== null) {
    assert(m.match === "y", "matched text is y");
    const groups: Map<string, string> = m.namedGroups;
    assert(groups.has("b"), "matched named capture b is present");
    assert(groups.get("b") === "y", "b captured y");
    assert(!groups.has("a"), "unmatched named capture a is omitted");
    assert(groups.size === 1, "only the participating name is in the map");
  }

  const m2: RegExpMatch | null = re.exec("x");
  assert(m2 !== null, "matched the first alternative");
  if (m2 !== null) {
    const groups2: Map<string, string> = m2.namedGroups;
    assert(groups2.has("a"), "matched named capture a is present");
    assert(groups2.get("a") === "x", "a captured x");
    assert(!groups2.has("b"), "unmatched named capture b is omitted");
  }
}
