// expect-error: rule out `v !== null` too
// `typeof v === "object"` alone leaves `object | null`.
type Rec = { [k: string]: unknown };
function isRec(v: unknown): v is Rec {
  return typeof v === "object";
}
export function main(): void {}
