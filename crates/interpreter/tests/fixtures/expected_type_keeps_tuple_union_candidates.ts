// Under an expected type that names enum members, an array of entries keeps
// each entry's literals, and a type argument binds the union of what each
// entry gives it, as tsc infers from each member of the entries' union.
enum Kind {
  Header = "Header",
  List = "List",
}
enum Style {
  Tree,
  Standard,
  Mini,
}

function pairs<K, V>(entries: [K, V][]): Map<K, V> {
  return new Map(entries);
}

function main(): void {
  const styles: Map<Kind, Style[]> = new Map([
    [Kind.Header, [Style.Standard, Style.Mini]],
    [Kind.List, [Style.Tree]],
  ]);
  assert(styles.get(Kind.Header)?.length === 2, "the header's styles");
  assert(styles.get(Kind.List)?.[0] === Style.Tree, "the list's style");

  const first: Map<Kind, Style> = pairs([
    [Kind.Header, Style.Tree],
    [Kind.List, Style.Mini],
  ]);
  first.set(Kind.Header, Style.Standard);
  assert(first.get(Kind.Header) === Style.Standard, "a wider value written back");
  assert(first.size === 2, "both entries");
}
