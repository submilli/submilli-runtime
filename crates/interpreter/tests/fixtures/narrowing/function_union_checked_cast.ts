// A union with a common closure slot still needs the same adapters as one function type.
type Numeric = (x: number) => number;
type Textual = (x: number) => string;
function main(): void {
  const zero: unknown = (): number => 7;
  const cast = zero as Numeric | Textual;
  assert(cast(1) === 7, "ignored trailing argument");
  const defaults: unknown = (x: number = 4): number => x;
  const defaultCast = defaults as (() => number) | (() => string);
  assert(defaultCast() === 4, "omitted default");
}
