type Num = number;
interface AliasBase { [key: string]: Num; x: Num; }
interface PlainBase { [key: string]: number; x: number; }
interface Aliases extends AliasBase, PlainBase {}
interface Wide { [key: string]: number | string; x: number | string; }
interface Narrow { [key: string]: number; x: number; }
interface NarrowFirst extends Narrow, Wide { x: number; }
interface OwnResolution extends Wide, Narrow { [key: string]: number; x: number; }
interface Mutable { [key: string]: number; }
interface ImmutableIndexBase { readonly [key: string]: number; }
interface MutableFirst extends Mutable, ImmutableIndexBase {}
interface ReadonlyField { readonly x: number; }
interface MutableField { x: number; }
interface ResolvedReadonly extends ReadonlyField, MutableField { readonly x: number; }
interface DataBaseA { [key: string]: unknown; data: ZDataA; }
interface DataBaseB { [key: string]: unknown; data: ZDataB; }
interface DataChild extends DataBaseA, DataBaseB {}
interface ZDataA { value: number; }
interface ZDataB { value: number; }
function main(): void {
  const aliases: Aliases = { x: 1 };
  assert(aliases.x === 1);
  const first: NarrowFirst = { x: 2 };
  first["other"] = 3;
  assert(first["other"] === 3);
  const resolved: OwnResolution = { x: 4 };
  resolved["other"] = 5;
  assert(resolved["other"] === 5);
  const mutable: MutableFirst = {};
  mutable["other"] = 6;
  assert(mutable["other"] === 6);
  const readonly: ResolvedReadonly = { x: 7 };
  assert(readonly.x === 7);
  const nested: DataChild = { data: { value: 8 } };
  assert(nested.data.value === 8);
}
