// A truthiness test that rules out every value leaves `never`, which is
// assignable to anything, for a local and for a module variable that no
// function assigns.
type Tag = "a" | "b";

let moduleTag: Tag = "a";

function falsyTag(tag: Tag): string {
  if (!tag) {
    const impossible: "unreachable" = tag;
    return impossible;
  }
  return tag;
}

function falsyModuleTag(): string {
  if (!moduleTag) {
    const impossible: "unreachable" = moduleTag;
    return impossible;
  }
  return moduleTag;
}

function main(): void {
  assert(falsyTag("b") === "b");
  assert(falsyModuleTag() === "a");
}
