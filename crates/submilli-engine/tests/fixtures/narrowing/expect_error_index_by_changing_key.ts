// A key that can change between the guard and the read, or a write to the
// element, leaves `obj[key]` with its declared type, as in TypeScript.
// expect-error: cannot read field `length` on `string | undefined`: the receiver can be `undefined`
// expect-error: cannot read field `length` on `string | undefined`: the receiver can be `undefined`
// expect-error-count: 2
function reassignedKey(obj: Record<string, string | undefined>, key: string): number {
  if (obj[key] !== undefined) {
    key = "other";
    return obj[key].length;
  }
  return 0;
}

function writtenElement(obj: Record<string, string | undefined>, key: string): number {
  if (obj[key] !== undefined) {
    obj[key] = undefined;
    return obj[key].length;
  }
  return 0;
}

function main(): void {
  console.log(reassignedKey({ k: "v" }, "k"), writtenElement({ k: "v" }, "k"));
}
