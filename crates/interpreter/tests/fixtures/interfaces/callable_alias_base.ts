type Fn<T> = (x: T) => T;
interface FnOp extends Fn<number> {}

type Callable = { (x: number): number };
interface CallableOp extends Callable {}
interface Derived extends CallableOp {}

function double(x: number): number { return x * 2; }
function applyFn(f: FnOp): number { return f(7); }
function applyDerived(f: Derived | null): number {
  return f === null ? 0 : f(8);
}

function main(): void {
  assert(applyFn(double) === 14, "call through a specialized function alias");
  assert(applyDerived(double) === 16, "inherit a callable object signature");
  assert(applyDerived(null) === 0, "nullable callable interface");
}
