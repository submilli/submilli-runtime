// A generic function value still has to fit the expected type: `id`
// instantiated at `number` returns a `number`, not a `string`.
// expect-error: expected `(arg0: number) => string`, got `Identity`
// expect-error-count: 1
type Identity = <T>(value: T) => T;

function main(): void {
  const id: Identity = <T,>(value: T): T => value;
  const wrong: (value: number) => string = id;
}
