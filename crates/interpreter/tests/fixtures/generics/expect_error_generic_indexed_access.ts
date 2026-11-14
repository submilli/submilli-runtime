// A type operator over a generic function type (`O["f"]`) evaluates to that
// type, so the rules for generic functions apply to it like to any other.
// expect-error: expected `string`, got `G`
// expect-error-count: 1
type G = <T>(k: string) => T | undefined;
type O = { f: G };
const load = <T,>(k: string): T | undefined => undefined;

function main(): void {
  const r: O["f"] = load;
  const s: string = r;
}
