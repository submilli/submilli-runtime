// expect-error: construct signatures are not supported in object types
// expect-error: declare an `interface` with the `new (…)` signature
// expect-error-count: 5
class Base {
  x: number = 1;
}

type Ctor = { new (): Base };
type GenericCtor = { new <T>(value: T): Base; label: string };
type SplitCtor = {
  new
  (): Base
};
type ReadonlyCtor = { readonly new(): Base };
let make = null as unknown as ({ new(x: number): Base });

function main(): void {}
