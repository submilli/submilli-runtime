// expect-error: the type guard assigns its parameter `v`
// After the write, the body's checks are about the written value, not the
// argument the caller's narrowing applies to.
type Rec = { [k: string]: unknown };
function isRec(v: unknown): v is Rec {
  v = JSON.parse('{"a":1}');
  return typeof v === "object" && v !== null;
}
export function main(): void {}
