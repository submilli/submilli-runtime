function present(value: unknown): string {
  if (value === null) {
    return "null";
  }
  if (value === undefined) {
    return "undefined";
  }
  const held: {} = value;
  return typeof held;
}

function withDefault(value: unknown): {} {
  return value ?? "default";
}

function main(): void {
  assert(present(null) === "null", "`null` is ruled out first");
  assert(present(undefined) === "undefined", "`undefined` is ruled out next");
  assert(present(0) === "number", "what is left is `{}`");
  assert(present({}) === "object", "objects are left too");
  assert(withDefault(null) === "default", "`??` replaces `null`");
  assert(withDefault(0) === 0, "`??` keeps a falsy value that is present");
}
