// A spread and `Object.assign` copy an interface's generic method, which a
// call through the copy then takes type arguments for (SUB-2137).
interface Identity {
  id<T>(value: T): T;
}

function copy(source: Identity): number {
  return { ...source }.id<number>(1);
}

function assigned(source: Identity): string {
  return Object.assign({}, source).id("x");
}

function direct(source: Identity): boolean {
  return source.id<boolean>(true);
}

function main(): void {
  const source: Identity = {
    id<T>(value: T): T {
      return value;
    },
  };
  assert(copy(source) === 1, "a spread keeps the generic method");
  assert(assigned(source) === "x", "Object.assign keeps the generic method");
  assert(direct(source), "a call through the interface");
}
