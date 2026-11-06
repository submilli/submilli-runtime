// Runtime-checked casts are an established Submilli extension to TypeScript.
let reads = 0;
class Source {
  get id_ok(): boolean { reads += 1; return true; }
  get unrelated(): string { throw new Error("unrelated getter must not run"); }
}
function main(): void {
  const source: unknown = new Source();
  const checked = source as Record<`id_${string}`, boolean>;
  assert(reads === 1, "only matching getters are checked");
  assert(checked.id_ok === true);
  let caught = false;
  try {
    const bad: unknown = { id_bad: "wrong", unrelated: true };
    const rejected = bad as Record<`id_${string}`, boolean>;
  } catch (e: Error) { caught = e.name === "TypeError" && e.message.includes("id_bad"); }
  assert(caught, "matching cast failure names its key");
}
