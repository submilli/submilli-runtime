// expect-error: expected 2 argument(s), got 1
// A union with a member that takes exactly two arguments is called through
// that member's signature, as in tsc, so the others' rest parameters don't
// let a call pass fewer or more.
type Rest = (a: string, ...rest: string[]) => void;
type Pair = (a: string, b: string) => void;
function callOne(f: Rest | Pair): void {
  f("a");
}
function main(): void {}
