// A switch on a `typeof` value needs no `default` and need not list every
// tag: it lists the tags its operand can have and falls through otherwise.
function describe(x: string | number): string {
  switch (typeof x) {
    case "string":
      return "text";
    case "number":
      return "count";
  }
  return "other";
}

function classify(x: unknown): string {
  const tag = typeof x;
  switch (tag) {
    case "number":
      return "n";
  }
  return tag;
}

function narrowed(x: unknown): string {
  const tag = typeof x;
  if (tag === "bigint") {
    return "big";
  }
  switch (tag) {
    case "number":
      return "n";
  }
  return "other";
}

function main(): void {
  assert(describe("a") === "text", "a string operand");
  assert(describe(2) === "count", "a number operand");
  assert(classify(1) === "n", "a listed tag");
  assert(classify("s") === "string", "an unlisted tag falls through");
  assert(narrowed(1) === "n", "a narrowed typeof value");
  assert(narrowed("s") === "other", "a narrowed typeof value falls through");
}
