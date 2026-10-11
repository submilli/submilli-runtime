// A static-dispatch binding such as `console` or `Number` is a real value: it
// converts to a string as in JavaScript, directly or through a generic, an
// array, `unknown` or `String(x)`, and every read is the same value.
function show<V>(v: V): string {
  return `${v}`;
}

const alias = console;

function main(): void {
  assert(show(console) === "[object console]", "console through a generic");
  assert(`${console}` === "[object console]", "interpolating console");
  assert(String(console) === "[object console]", "String(console)");
  assert(
    [Number, Number].join(",") ===
      "function Number() { [native code] },function Number() { [native code] }",
    "joining constructors",
  );
  assert(`${[Map]}` === "function Map() { [native code] }", "a constructor in an array");
  assert(show(Temporal.Instant) === "function Instant() { [native code] }", "a namespaced constructor");

  const value: unknown = console;
  assert(value === console && value !== null, "an erased binding keeps its identity");
  const maybe: Console | null = console;
  assert(maybe !== null, "a binding is not null");
  assert(JSON.stringify([console]) === "[{}]", "serializing a binding");
  assert(JSON.stringify({ c: console, n: 1 }) === '{"c":{},"n":1}', "a binding nested in an object");
  alias.log("calls through an alias still work");
}
