// `typeof x` read as a value is the union of the tags `typeof` returns, as tsc
// types it, so a binding of it takes any tag and a switch on it can cover them.
function describe(value: number | string): string {
  const tag = typeof value;
  switch (tag) {
    case "string":
    case "number":
    case "bigint":
    case "boolean":
    case "symbol":
    case "undefined":
    case "object":
    case "function":
      return tag;
  }
}

function main(): void {
  let t = 10;
  let tag = typeof t;
  assert(tag === "number", tag);
  tag = "object";
  assert(tag === "object", tag);
  assert(describe("s") === "string", "a switch on a typeof value");
  assert(`${typeof t}!` === "number!", "a typeof in a template");
}
