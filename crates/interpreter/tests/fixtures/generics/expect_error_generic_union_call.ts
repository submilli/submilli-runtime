// A union with a generic function member has no combined signature to call
// through: the member's type arguments would go unbound.
// expect-error: cannot call
// expect-error-count: 1
const echo = <T,>(x: T): T => x;
const len = (x: string): number => x.length;

function main(): void {
  const flag: boolean = true;
  const u = flag ? echo : len;
  const r = u("abc");
}
