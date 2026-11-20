// expect-error: a local `v` shadows the type guard's parameter at this return
// The body's checks narrow the local, so they prove nothing about the
// argument the caller's narrowing applies to.
type Rec = { [k: string]: unknown };
function isRec(v: unknown): v is Rec {
  {
    const v: Rec = { a: 1 };
    return true;
  }
}
export function main(): void {}
