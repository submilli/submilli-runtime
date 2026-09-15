// expect-error: `never` cannot be used as a type argument to method `pick`
// The method call site gets the same rejection as the plain function one.
interface Chooser {
  pick<T>(a: T): T;
}

function use(c: Chooser): void {
  c.pick<never>(1);
}

function main(): void {
  assert(1 === 1, "ok");
}
