// Two bases with call signatures that disagree make no interface, as two
// bases with disagreeing members don't. TypeScript merges them into an
// overloaded call signature, which an interface can't declare here.
// expect-error: incompatible inherited call signature
// expect-error-count: 1
interface ToNumber {
  (n: number): number;
}
interface ToString {
  (n: number): string;
}
interface Both extends ToNumber, ToString {}
interface Same extends ToNumber, ToNumber {}
function main(): void {}
