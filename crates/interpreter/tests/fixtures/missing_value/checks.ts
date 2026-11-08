// SUB-2148: each check decides a missing read, for an array element, a record
// key (`r[k]` and `rec.foo`) and a regex group, read directly or through a
// local it initializes. A present value passes through every check.
// expect-error-count: 0
function main(): void {
  const a: string[] = ["x"];
  const r: Record<string, string> = { k: "v" };
  const m = /(a)|(b)/.exec("b");
  if (m === null) throw new Error("no match");
  const i = 5;
  const key = "absent";

  // `??`
  assert((a[i] ?? "d") === "d", "array ??");
  assert((a[0] ?? "d") === "x", "present array ??");
  assert((r[key] ?? "d") === "d", "record ??");
  assert((r.absent ?? "d") === "d", "dotted record ??");
  assert((r.k ?? "d") === "v", "present record ??");
  assert((m[1] ?? "d") === "d", "group ??");
  assert((m[2] ?? "d") === "b", "present group ??");

  // `===` and `!==`, with `undefined` or any other value
  assert(a[i] === undefined, "array ===");
  assert(!(r[key] !== undefined), "record !==");
  assert(m[1] === undefined && m[2] !== undefined, "group === and !==");
  assert(!(a[i] === "x") && r[key] !== "v", "=== and !== with a value");
  assert(m[1] !== "a" && m[2] === "b", "group === and !== with a value");

  // `!`, `||`, and the tests of `if`, `while` and `?:`
  assert(!a[i], "array !");
  assert((r[key] || "or") === "or", "record ||");
  assert((r.k || "or") === "v", "present record ||");
  let tested = "";
  if (m[1]) tested += "1";
  if (m[2]) tested += "2";
  assert(tested === "2", "group if");
  let loops = 0;
  while (a[loops]) loops += 1;
  assert(loops === 1, "array while");
  assert((r[key] ? "yes" : "no") === "no", "record ?:");
  assert((r[key] && r[key].length > 0 ? "yes" : "no") === "no", "&& in a test");

  // `?.` and `typeof`
  const objects: { name: string }[] = [{ name: "n" }];
  assert((objects[i]?.name ?? "d") === "d", "array ?.");
  assert((objects[0]?.name ?? "d") === "n", "present array ?.");
  assert(typeof a[i] === "undefined", "array typeof");
  assert(typeof r[key] === "undefined" && typeof r.k === "string", "record typeof");
  assert(typeof m[1] === "undefined", "group typeof");

  // `switch` compares with `===`.
  let switched = "";
  switch (a[i]) {
    case "x":
      switched = "x";
      break;
    default:
      switched = "default";
  }
  assert(switched === "default", "switch goes to default");

  // A `?.` after a step that may be missing checks it.
  const nested: { items: { name: string }[] }[] = [{ items: [] }];
  assert((nested[0]?.items[0]?.name ?? "none") === "none", "?. after a missing step");

  // The same checks through a local the read initializes.
  const v = r[key];
  assert((v ?? "d") === "d", "local ??");
  assert(v === undefined, "local ===");
  assert(!v, "local !");
  assert(typeof v === "undefined", "local typeof");
  const present = r.k;
  assert((present ?? "d") === "v", "present local");
  const element = objects[i];
  assert((element?.name ?? "d") === "d", "local ?.");
  const group = m[1];
  assert((group || "or") === "or", "local group ||");
}
