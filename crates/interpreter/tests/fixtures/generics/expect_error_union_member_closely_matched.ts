// A union argument's member that matches a parameter member of the same
// class, interface or array kind pairs with it rather than going to the
// parameter's type variable, so a mismatch inside it is reported, as in tsc.
// This holds when absorbing another member already bound the type variable,
// or when a later argument or object-literal field binds it, for function and
// constructor calls alike. Each mismatching argument is reported once.
// expect-error: expected `number`, got `string`
// expect-error: expected `number`, got `string`
// expect-error: expected `number`, got `string`
// expect-error: type parameter `T` already bound to `number`, cannot bind to `string`
// expect-error: expected `number`, got `boolean`
// expect-error: expected `number`, got `boolean`
// expect-error: expected `number`, got `string`
// expect-error-count: 7
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

function unwrap<T>(x: T | Box<T>): number {
  return 0;
}

function eitherBox(flag: boolean): Box<number> | Box<string> {
  return flag ? new Box(1) : new Box("s");
}

function laterDecides<T>(x: T | Box<number>, y: T): number {
  return 0;
}

function textOrFlagBox(flag: boolean): Box<string> | Box<boolean> {
  return flag ? new Box("a") : new Box(true);
}

function fromFields<T>(holder: { value: T | Box<number>; last: T; use: (each: T) => number }): T {
  return holder.last;
}

class Pair<T> {
  constructor(first: T | Box<number>, second: T) {}
}

function main(): void {
  fromBox(boxOrText(true));
  laterDecides(textOrFlagBox(true), new Box("s"));
  new Pair(textOrFlagBox(true), new Box("s"));
  fromFields({ value: textOrFlagBox(true), last: new Box(true), use: (each) => 1 });
  unwrap(eitherBox(false));
  fromList(listOrFlag(true));
  fromHolder(holderOrFlag(true));
}
