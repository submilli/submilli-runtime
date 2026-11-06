function fail(): string { throw new Error("must not run"); }
function main(): void {
  const s: string = "value";
  assert((s ?? fail()) === "value");
  assert(("literal" ?? fail()) === "literal");
  try { throw new Error("caught"); } catch (e) {
    assert(e.message === "caught");
    try { throw e; } catch (again) { assert(again.message === "caught"); }
  }
}
