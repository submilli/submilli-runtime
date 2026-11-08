function asArray<T extends string>(value: T): T[] { return [value]; }
function callable<F extends (...args: never[]) => unknown>(fn: F): F { return fn; }
function invoke<F extends () => number>(fn: F): number { return fn(); }
function first<T extends number[]>(values: T): number { return values[0]; }
function local<T extends { id: number }>(value: T): number {
  type Wrapped<U extends T> = { item: U };
  const wrapped: Wrapped<T> = { item: value };
  return wrapped.item.id;
}
interface Recursive<T> { next?: T; id: number; }
function recursive<T extends Recursive<T>>(value: T): number { return value.id; }
function absentRecursive<T extends Recursive<T>>(): T | undefined { return undefined; }
function absentString<T extends string>(): T | undefined { return undefined; }
function acceptOptional<T extends string>(value: T | undefined): T | undefined { return value; }
function callback<T extends { id: number }>(read: (value: T) => number): void {}
function recursiveCallback<T extends Recursive<T>>(read: (value: T) => number): void {}
function widen<T extends string | number>(value: T): string | number { return value; }
function optionalFirst<T extends number[]>(values: T): number { return values?.[0]; }
function nullable<T extends { id: number }>(value: T | undefined): number | undefined {
  return value?.id;
}
function unionReceiver<T extends { id: number }>(value: T | { id: number }): number {
  return value.id;
}
function optionalNested<T extends { item: U }, U extends { id: number }>(value: T): number {
  return value?.item?.id;
}
function mutate<T extends number[], U extends { value: number }>(values: T, object: U): void {
  values[0] = 6;
  values[0]++;
  object.value++;
}
interface StringValue { value: string; }
interface ConstrainedValue<T extends string> extends StringValue { value: T; }
class ConstrainedClass<T extends string> implements StringValue {
  value: T;
  constructor(value: T) { this.value = value; }
}
function main(): void {
  const literal: "hello"[] = asArray("hello");
  assert(literal[0] === "hello", "primitive constraint preserves literal");
  const fn = callable((value: number): number => value + 1);
  assert(fn(41) === 42, "callable constraint accepts a fixed signature");
  assert(invoke(() => 42) === 42, "constrained function call");
  assert(first([42]) === 42, "constrained array index");
  assert(local({ id: 42 }) === 42, "lifted local type preserves constraints");
  assert(recursive({ id: 42 }) === 42, "recursive structural constraint");
  assert(absentRecursive() === undefined, "uninferred recursive bound");
  assert(acceptOptional(undefined) === undefined, "concrete union member does not violate bound");
  const acceptsUnknown: (value: unknown) => boolean = value => value === undefined;
  assert(acceptsUnknown(absentString()), "unknown context keeps constraint fallback");
  callback(value => value.id);
  recursiveCallback(value => value.id);
  assert(widen(42) === 42, "parameter assignable to its union bound");
  assert(optionalFirst([42]) === 42, "optional index through constraint");
  assert(nullable({ id: 42 }) === 42 && nullable(undefined) === undefined, "nullable parameter constraint");
  assert(unionReceiver({ id: 42 }) === 42, "constraint in union receiver");
  assert(optionalNested({ item: { id: 42 } }) === 42, "constraint mid-chain");
  const values = [1];
  const object = { value: 41 };
  mutate(values, object);
  assert(values[0] === 7 && object.value === 42, "constraint writes and increments");
  const inherited: ConstrainedValue<string> = new ConstrainedClass("value");
  assert(inherited.value === "value", "constraints in inheritance proofs");
}
