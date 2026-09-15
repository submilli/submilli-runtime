// A narrowed path with an INDEX step reached through another narrowing. Its
// source can't be rebuilt from declared types, so the engine must refuse the
// narrowing (and say so) rather than emit a region reading a shadow whose
// scope ended with the `&&` RHS.
interface Leaf {
  c: string | null;
}

function guarded(arr: Array<Leaf> | null): string {
  // the `arr !== null` narrowing itself survives and is what makes the
  // index read legal at all
  if (arr !== null && arr[0].c !== null) {
    return "guarded";
  }
  return "none";
}

function hoisted(arr: Array<Leaf> | null): string {
  // the prescribed workaround: bind the element, then narrow that
  if (arr !== null) {
    const c = arr[0].c;
    if (c !== null) {
      return c;
    }
  }
  return "none";
}

function main(): void {
  assert(guarded([{ c: "x" }]) === "guarded");
  assert(guarded([{ c: null }]) === "none");
  assert(guarded(null) === "none");

  assert(hoisted([{ c: "x" }]) === "x");
  assert(hoisted([{ c: null }]) === "none");
  assert(hoisted(null) === "none");
}
