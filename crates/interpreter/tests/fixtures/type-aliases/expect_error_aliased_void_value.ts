// expect-error: cannot bind a `void` value
// expect-error: `JSON.stringify(x)` requires a non-`void` argument
// The other half of peeling `void`: an alias of `void` is still not a value, so
// every rejection that names `void` has to fire through the alias too.
type V = void;

function nothing(): V {
  console.log("nothing");
}

function main(): void {
  const bound: V = nothing();
  console.log(JSON.stringify(nothing()));
}
