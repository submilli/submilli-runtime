// A property access on an object-shaped receiver emits the accessor branch too
// — a dispatch to the synthetic `get p` / `set p` method — and that branch is
// emitted on the field-name scan alone, whether or not any accessor exists.
// The `in` tests are what mint the `get p` / `set p` name globals.
interface Thing {
  p: string;
}

function main(): void {
  const t: Thing = { p: "hi" };
  const u: unknown = t;

  assert(!("get p" in u), "no accessor named `get p`");
  assert(!("set p" in u), "no accessor named `set p`");

  assert(t.p === "hi", "data-field read still reads the payload slot");
  t.p = "bye";
  assert(t.p === "bye", "data-field write still writes the payload slot");
}
