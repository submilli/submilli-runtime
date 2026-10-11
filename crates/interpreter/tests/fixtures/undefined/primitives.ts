function identity<T>(value: T): T { return value; }
function shadow(undefined: string): string { return undefined; }
function absent(): unknown { return; }

function main(): void {
  const value: undefined = undefined;
  assert(value !== null, "undefined and null remain distinct");
  assert(value === undefined, "undefined equals itself");
  assert(shadow("local") === "local", "undefined value name may be shadowed");
  assert(identity(value) === undefined, "undefined survives erasure");
  assert(absent() === undefined, "bare completion produces undefined");
  assert(typeof value === "undefined");
  const tag = typeof identity<unknown>(undefined);
  assert(tag === "undefined", "typeof is a value expression");
  assert(typeof null === "object");
  assert(`${value}` === "undefined", "undefined string conversion");
  let calls = 0;
  const bump = (): number => { calls++; return calls; };
  assert((void bump()) === undefined);
  assert(calls === 1, "void evaluates exactly once");
  assert(typeof bump() === "number");
  assert(calls === 2, "typeof evaluates exactly once");
}
