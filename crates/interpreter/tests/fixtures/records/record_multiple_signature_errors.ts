// expect-error: `number` index type `number` is not assignable to `string` index type `string`
// expect-error: `ab${string}`` index type `string` is not assignable to ``a${string}`` index type `number`
// expect-error: duplicate index signature for type `string`
// expect-error: incompatible inherited index signatures for key
// expect-error: index key `string` does not satisfy
// expect-error: expected `string`, got `number`
// expect-error: `number` index type `string` is not assignable to `string` index type `T`
// expect-error: expected `number`, got `string`
// expect-error: expected `string`, got `0`
// expect-error: cannot assign to readonly property `a1`
// expect-error: expected `{ [key: `a${string}`]: number }`, got `Instance`
// expect-error: expected `Signatures`, got `Instance`
// expect-error: index signatures `[key: `a${string}`]` and `[key: `${string}z`]` overlap without one key containing the other
// expect-error: `[key: `${string}y`]` and `[key: `b${string}`]` overlap
// expect-error: `[key: `${string}x`]` and `[key: `c${string}`]` overlap
// expect-error: ``dd${string}`` index type `{ b: string }` is not assignable to ``d${string}`` index type `{ a: number }`
// expect-error: `[key: `e${number}`]` and `[key: `${string}f`]` overlap
// expect-error: `[key: `g${string}`]` and `[key: `${string}w`]` overlap
// expect-error: `[key: `${string}v`]` and `[key: `h${string}`]` overlap
// expect-error-count: 22
// Signatures must agree where their keys nest (tsc's TS2413), a key appears
// once (TS2374), and bases agree on a key they share.
type Inconsistent = { [k: string]: string; [i: number]: number };
interface Base { [k: `a${string}`]: number }
interface Narrower extends Base { [k: `ab${string}`]: string }
type Duplicate = { [k: string]: number; [j: string]: number };
interface Numbers { [k: `a${string}`]: number }
interface Strings { [k: `a${string}`]: string }
interface Both extends Numbers, Strings {}
type Disjoint = { [k: `a${string}`]: number; [k: `b${string}`]: string };
type ReadonlyMix = { readonly [k: `a${string}`]: number; [k: `ab${string}`]: number };
// Checked once `T` is known to be any type, as tsc checks it.
interface Generic<T> { [k: string]: T; [i: number]: string }
// A write with a key union must fit the signature of every member.
function put(o: Record<string, number>, k: string | number): void {
  o[k] = "s";
}
function setAll(o: Disjoint, k: `a${string}` | `b${string}`): void {
  o[k] = 0;
}
// A dynamic key may name a narrower signature's keys too, so a write must fit
// it, where tsc checks only the `string` signature.
function putWide(o: { [k: string]: number | string; [k: `a${string}`]: number }, k: string): void {
  o[k] = "s";
}
// A key that overlaps a signature without nesting may name its keys too.
function putOverlap(o: { [k: string]: number | string; [k: `${string}z`]: number }, k: `a${string}`): void {
  o[k] = "s";
}
// A key parameter's bound may name a declared field.
function putField<K extends "x" | `a${string}`>(o: { x: number; [k: `a${string}`]: string }, k: K): void {
  o[k] = "s";
}
// Keys that overlap without nesting would read an intersection of unrelated
// values, which this front end doesn't support: in a declaration, through
// inheritance, or by Object.assign.
type Crossing = { [k: `a${string}`]: number; [k: `${string}z`]: string };
interface ByPrefix { [k: `b${string}`]: number }
interface BySuffix { [k: `${string}y`]: string }
interface Crossed extends ByPrefix, BySuffix {}
function crossAssign(prefixed: Record<`c${string}`, number>, suffixed: Record<`${string}x`, string>): void {
  const crossed = Object.assign({}, prefixed, suffixed);
}
// A number hole takes hex text such as "0xff", which ends with a hex letter.
type HexEnd = { [k: `e${number}`]: number; [k: `${string}f`]: string };
// Object values that overlap intersect field by field, which needs a type this
// front end lacks, even where one fits the other.
type ObjectValues = { [k: `g${string}`]: { x: number; y?: string }; [k: `${string}w`]: { x: number } };
// Each alternative of a union result is checked.
function unionAssign(target: Record<`h${string}`, string> | { kind: "none" }, suffixed: Record<`${string}v`, number>): void {
  const merged = Object.assign(target, suffixed);
}
// Object.assign refuses nested signatures whose values don't nest, as a
// declaration would.
function nestAssign(wide: Record<`d${string}`, { a: number }>, narrow: Record<`dd${string}`, { b: string }>): void {
  const nested = Object.assign({}, wide, narrow);
}
// A class instance has no signature to tell which members a key names.
class Instance {
  a1: number = 1;
  name: string = "x";
}
interface Signatures { [k: string]: number }
function main(): void {
  const instance: { [k: `a${string}`]: number } = new Instance();
  const asInterface: Signatures = new Instance();
  const mix: ReadonlyMix = {};
  mix.a1 = 2;
  const o: Disjoint = { a1: 1 };
  const k: string = "a1";
  const read = o[k];
  o.b9 = 3;
}
