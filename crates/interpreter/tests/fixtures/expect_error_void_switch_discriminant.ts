// An empty `switch` has no case label to raise a comparison error first, so
// the discriminant needs its own screen — `if`/`while`/`for` conditions
// already have one.
// expect-error: cannot compare `void`: a `switch` discriminant must be a value
function f(): void {}

function main(): void {
  switch (f()) {
  }
}
