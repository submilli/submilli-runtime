// A key that can change between the guard and the read, or a write to the
// element, leaves `obj[key]` with its declared type, as in TypeScript.
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error-count: 2
function reassignedKey(obj: Record<string, string | null>, key: string): number {
  if (obj[key] !== null) {
    key = "other";
    return obj[key].length;
  }
  return 0;
}

function writtenElement(obj: Record<string, string | null>, key: string): number {
  if (obj[key] !== null) {
    obj[key] = null;
    return obj[key].length;
  }
  return 0;
}

function main(): void {
  console.log(reassignedKey({ k: "v" }, "k"), writtenElement({ k: "v" }, "k"));
}
