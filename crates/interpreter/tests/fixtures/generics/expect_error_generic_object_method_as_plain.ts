// An object's generic method can't stand for a method without type
// parameters once the object has its own type (tsc accepts).
// expect-error: expected `NumLoader`, got `Loader`
// expect-error-count: 1
interface NumLoader {
  load(key: string): number | undefined;
}
interface Loader {
  load<T>(key: string): T | undefined;
}

function main(): void {
  const literal: Loader = {
    load<T>(key: string): T | undefined {
      return undefined;
    },
  };
  const plain: NumLoader = literal;
}
