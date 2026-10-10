interface View { value: string | null; }
class Source { value: string = "ok"; }
class GetterSource {
  calls: number = 0;
  get value(): string { this.calls = this.calls + 1; return "ok"; }
}
function unionSource(flag: boolean): { x: string; n: number } | { x: string; b: boolean } {
  if (flag) { return { x: "ok", n: 1 }; }
  return { x: "ok", b: true };
}
function tupleSource(flag: boolean): [string, number] | [string, boolean] {
  if (flag) { const pair: [string, number] = ["ok", 1]; return pair; }
  const pair: [string, boolean] = ["ok", true];
  return pair;
}
function main(): void {
  const narrowedSource: { x: string | null } = { x: null };
  narrowedSource.x = "ok";
  let { x: narrowedCopy }: { x: string | null } = narrowedSource;
  assert(narrowedCopy.length === 2, "source field assignment flow");
  const guarded: { x: string | null } = { x: "yes" };
  if (guarded.x !== null) {
    const { x: guardedCopy }: { x: string | null } = guarded;
    assert(guardedCopy.length === 3, "source field guard flow");
  }
  let { value: data }: View = new Source();
  assert(data.length === 2, "class source");
  data = null;
  const getter = new GetterSource();
  const { value: snapshot }: View = getter;
  assert(snapshot.length === 2 && getter.calls === 1, "getter snapshot");
  const { x: union }: { x: string | null } = unionSource(true);
  assert(union.length === 2, "union source");
  const [first]: [string | null, number | boolean] = tupleSource(false);
  assert(first.length === 2, "tuple union source");
  let { inline }: { inline: string | null } = { inline: "yes" };
  assert(inline.length === 3, "contextual literal");
  inline = null;
  const obj = { x: "ok" };
  let { x }: { x: string | null } = obj;
  assert(x.length === 2, "initializer narrows destructured field");
  x = null;
  assert(x === null, "annotation permits reassignment");
  x = "again";
  assert(x.length === 5, "reassignment narrows");
  const { x: renamed }: { readonly x: string | null } = obj;
  assert(renamed.length === 2, "readonly renamed field");
  const tuple: [string, number] = ["yes", 3];
  let [text, count]: [string | null, number | null] = tuple;
  assert(text.length === 3 && count + 1 === 4, "tuple source flow");
  text = null;
  count = null;
  assert(text === null && count === null, "tuple storage types");
}
