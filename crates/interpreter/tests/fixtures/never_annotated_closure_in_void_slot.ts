// A closure annotated `: never` (or an alias of it) passed where a `void`
// returning function is expected. `never` occupies a value slot, so the
// closure would lower with a result the `void` funcref signature has no room
// for; the contextual `void` hint is adopted instead, exactly as it already is
// for an *unannotated* diverging body, which `void_hint_diverging_closure.ts`
// covers.
type N = never;

function boom(): never {
  throw new Error("boom");
}

function runVoid(f: (x: number) => void): void {
  f(1);
}

function main(): void {
  let caught = "";
  try {
    runVoid((x: number): never => boom());
  } catch (e) {
    caught = caught + "bare;";
  }
  assert(caught === "bare;", "`: never` annotation into a void slot");

  try {
    runVoid((x: number): N => boom());
  } catch (e) {
    caught = caught + "alias;";
  }
  assert(caught === "bare;alias;", "alias-of-never annotation into a void slot");

  // The pre-existing unannotated and `: void` forms still work.
  try {
    runVoid((x: number): void => boom());
  } catch (e) {
    caught = caught + "void;";
  }
  assert(caught === "bare;alias;void;", "`: void` annotation still works");
}
