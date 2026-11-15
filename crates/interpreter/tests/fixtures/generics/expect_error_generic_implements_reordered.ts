// A class implementing a generic interface method must declare the same type
// parameters in the same roles (tsc accepts this reordering).
// expect-error: class `Swapped` does not implement `Pair`: member `load` has an incompatible signature
// expect-error-count: 1
interface Pair {
  load<A, B>(key: string, other: B): A | undefined;
}
class Swapped implements Pair {
  load<Y, X>(key: string, other: Y): X | undefined {
    return undefined;
  }
}

function main(): void {}
