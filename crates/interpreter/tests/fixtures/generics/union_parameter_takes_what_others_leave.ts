// A type parameter in a union parameter takes the members of a union argument
// that the parameter's other members don't, as in tsc: `T | null` with a
// `Mode | null` argument binds `T` to `Mode`, as does an index signature's
// `A | number` with a `Mode` field.
type Mode = "on" | "off";

function orElse<T>(value: T | null, fallback: T): T {
  return value === null ? fallback : value;
}

function unwrap<T>(value: T | null): T[] {
  return value === null ? [] : [value];
}

function lookup(key: number): Mode | null {
  return key > 0 ? "on" : null;
}

function lookupEither(key: number): string | number | null {
  return key > 0 ? 1 : null;
}

function firstNamed<A>(record: { [key: string]: A | number }): A | null {
  return null;
}

function main(): void {
  const found: Mode[] = unwrap(lookup(1));
  const chosen: Mode = orElse(lookup(0), "off");
  const either: (string | number)[] = unwrap(lookupEither(1));
  let mode: Mode = "on";
  if (found.length > 5) {
    mode = "off";
  }
  const named: Mode | null = firstNamed({ z: mode });
  assert(found.join(",") === "on" && chosen === "off", "a nullable union");
  assert(either.join(",") === "1" && named === null, "several members");
}
