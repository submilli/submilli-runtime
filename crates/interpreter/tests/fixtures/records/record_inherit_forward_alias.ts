type Alias = ZBase;
interface AChild extends Alias { total: number; }
interface ZBase { [key: string]: number; }
type Identity<T> = T;
type GenericAlias<T> = ZGeneric<T>;
interface GenericChild extends GenericAlias<number> { total: number; }
interface IdentityChild extends Identity<ZBase> { total: number; }
interface ZGeneric<T> { [key: string]: T; }
function main(): void {
  const c: AChild = { total: 1 };
  c["other"] = 2;
  assert(c["other"] === 2);
  const generic: GenericChild = { total: 3 };
  generic["other"] = 4;
  assert(generic["other"] === 4);
  const identity: IdentityChild = { total: 5 };
  identity["other"] = 6;
  assert(identity["other"] === 6);
}
