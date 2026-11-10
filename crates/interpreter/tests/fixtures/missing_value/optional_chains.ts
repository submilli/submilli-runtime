// SUB-2148: `?.` checks a base that may be missing, but the chain keeps the
// type tsc gives it. So the chain is itself a missing value: a check around
// it sees `undefined`, a local it initializes holds it maybe missing, and any
// other read throws.
type Side = "left" | "right";

function message(read: () => string): string {
  try {
    return "=" + read();
  } catch (e: MissingValueError) {
    return e.message;
  }
}

interface Repo {
  items(): { name: string }[];
}

function main(): void {
  const arr: { tag: Side }[] = [{ tag: "left" }, { tag: "left" }];

  assert(arr[5]?.tag === undefined, "`=== undefined` around the chain");
  assert(!(arr[5]?.tag === "left"), "`===` with a value around the chain");
  assert((arr[5]?.tag ?? "none") === "none", "`??` around the chain");
  let i = 0;
  while (arr[i]?.tag === "left") i += 1;
  assert(i === 2, "a loop that walks off the end stops");

  const tag: Side = arr[1]?.tag;
  assert(tag === "left", "the chain has tsc's type");
  assert(
    message(() => { const t = arr[5]?.tag; return t; }) === "`t` is missing: `arr[5]?.tag` on line 32 found no value",
    "a local holds the chain maybe missing",
  );
  assert(message(() => { const t = arr[5]?.tag; return t ?? "none"; }) === "=none", "a check on that local");
  assert(
    message(() => { const t: Side = arr[5]?.tag; return t; }).startsWith("index 5 is out of range for `arr`"),
    "a place of type `Side` reads the base",
  );

  // A method call in the chain folds with its callee; the steps after it
  // still reach the check.
  const repos: Repo[] = [{ items: () => [] }];
  assert((repos[0]?.items()[3] ?? { name: "d" }).name === "d", "a missing last step after a method call");
  assert((repos[0]?.items()[3]?.name ?? "d") === "d", "a missing step after a method call");
}
