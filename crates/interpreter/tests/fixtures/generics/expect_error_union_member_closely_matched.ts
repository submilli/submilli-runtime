// A union argument's member that matches a parameter member of the same
// class, interface or array kind pairs with it rather than going to the
// parameter's type variable, so a mismatch inside it is reported, as in tsc.
// expect-error: expected `number`, got `string`
// expect-error: expected `number`, got `string`
// expect-error: expected `number`, got `string`
// expect-error-count: 3
class Box<A> {
  constructor(public v: A) {}
}

interface Holder<A> {
  v: A;
}

function fromBox<T>(x: T | Box<number>): number {
  return 0;
}

function fromList<T>(x: T | number[]): number {
  return 0;
}

function fromHolder<T>(x: T | Holder<number>): number {
  return 0;
}

function boxOrText(flag: boolean): Box<string> | string {
  return flag ? new Box("abc") : "s";
}

function listOrFlag(flag: boolean): string[] | boolean {
  return flag ? ["q"] : false;
}

function holderOrFlag(flag: boolean): Holder<string> | boolean {
  return flag ? { v: "q" } : false;
}

function main(): void {
  fromBox(boxOrText(true));
  fromList(listOrFlag(true));
  fromHolder(holderOrFlag(true));
}
