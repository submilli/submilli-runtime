type V = void;
function nothing(): V {}
function main(): void {
  const bound: V = nothing();
  assert(bound === undefined, "aliased void binding stores undefined");
  assert(JSON.stringify(nothing()) === undefined, "JSON root undefined stays undefined");
}
