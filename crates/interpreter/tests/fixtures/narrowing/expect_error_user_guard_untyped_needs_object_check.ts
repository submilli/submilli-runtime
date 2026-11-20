// expect-error: narrow `v` to an object with `typeof v === "object" && v !== null`
type Rec = { [k: string]: unknown };
function isRec(v: unknown): v is Rec {
  return v !== null;
}
export function main(): void {}
