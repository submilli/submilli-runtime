// The same annotation reached through a type alias.
// expect-error: `void` cannot be a parameter type — it has no values
type Consume = (x: void) => number;

function main(): void {
  const f: Consume = (x) => 1;
}
