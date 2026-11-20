// expect-error: the type guard assigns its parameter `v`
// A closure's write counts too: the closure may run before the return.
type Rec = { [k: string]: unknown };
function isRec(v: unknown): v is Rec {
  const reset = (): void => {
    v = 1;
  };
  return typeof v === "object" && v !== null;
}
export function main(): void {}
