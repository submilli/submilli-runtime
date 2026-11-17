interface OptionalAccessor { p?: number }
let getterCalls = 0;
function removeAccessor(o: OptionalAccessor): boolean { return delete o.p; }
class InheritedAccessor { get p(): number { getterCalls++; return 3; } }
class InheritedMethod { p(): number { return 7; } }
function eraseNext(o: { next?: unknown }): boolean { return delete o.next; }
function main(): number {
  const inherited: OptionalAccessor = new InheritedAccessor();
  if (!removeAccessor(inherited) || getterCalls !== 0) return 1;
  if (inherited.p !== 3 || !("p" in inherited)) return 2;
  const method: { p?: () => number } = new InheritedMethod();
  const alias = method;
  if (!delete method.p || alias.p === undefined || alias.p() !== 7) return 3;
  const iterator = [1, 2].values();
  Object.assign(iterator, { next: 1 });
  Object.assign(iterator, { next: 2 });
  const copy = { ...iterator };
  if (!eraseNext(copy) || "next" in copy) return 4;
  if (!eraseNext(iterator) || !("next" in iterator) || Object.hasOwn(iterator, "next")) return 5;
  if (iterator.next().value !== 1 || Object.keys(iterator).length !== 0) return 6;
  if (!eraseNext(iterator) || iterator.next().value !== 2) return 7;
  return 0;
}
