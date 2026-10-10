// `delete` is rejected by name. An object can't record one of its fields as
// absent unless it was built with that field optional (SUB-971). `delete(…)`
// is still a call, `delete as T` is still a cast of a value named `delete`, and
// `delete` is still a member name. The operand is still checked, and a binding
// named `as` is an operand, not a cast.
// expect-error-count: 5
// expect-error: the `delete` operator is not supported
// expect-error: the `delete` operator is not supported
// expect-error: the `delete` operator is not supported
// expect-error: the `delete` operator is not supported
// expect-error: unresolved identifier `missing`

type Draft = { id: number; note?: string };

class Store {
  delete(key: string): boolean {
    return key.length > 0;
  }
}

function exported(): (key: string) => boolean {
  const delete = (key: string): boolean => key.length > 0;
  return delete as (key: string) => boolean;
}

function main(): void {
  const d: Draft = { id: 1, note: "x" };
  delete d.note;
  const removed: boolean = delete d.note;
  delete missing.note;
  const as: Draft = { id: 2, note: "a" };
  delete as.note;
  const m = new Map<string, number>();
  m.set("k", 1);
  console.log(removed, m.delete("k"), new Store().delete("k"));
}
