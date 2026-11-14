// A class's generic method can't stand for a method without type parameters:
// a call through the plain interface has no type arguments to pass (tsc
// accepts).
// expect-error: expected `NumLoader`, got `Store`
// expect-error-count: 1
interface NumLoader {
  load(key: string): number | undefined;
}
class Store {
  load<T>(key: string): T | undefined {
    return undefined;
  }
}

function main(): void {
  const loader: NumLoader = new Store();
}
