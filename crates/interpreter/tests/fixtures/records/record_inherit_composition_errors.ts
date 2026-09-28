// expect-error: incompatible inherited string index signatures
// expect-error: cannot assign through readonly index signature
// expect-error: incompatible inherited member
interface Wide { [key: string]: number | string; }
interface Narrow { [key: string]: number; }
interface BadOrder extends Wide, Narrow {}
interface ImmutableIndexBase { readonly [key: string]: number; }
interface Mutable { [key: string]: number; }
interface ReadonlyFirst extends ImmutableIndexBase, Mutable {}
interface A { [key: string]: unknown; data: ZNumber; }
interface B { [key: string]: unknown; data: ZString; }
interface BadNested extends A, B {}
interface ZNumber { value: number; }
interface ZString { value: string; }
interface NumberField { x: number; }
interface StringField { x: string; }
interface BadOverride extends NumberField, StringField { x: number; }
function main(): void {
  const readonly: ReadonlyFirst = {};
  readonly["other"] = 1;
}
