// Variables destructured from a union narrow each other as in tsc: through a
// `switch` default, past a guard on the source, by tuple elements, across
// tuples of different lengths, and from an initializer that leaves out an
// optional element.
type Action = { kind: "A"; payload: number } | { kind: "B"; payload: string };
type Maybe = { kind: "A"; payload: number | undefined } | { kind: "B"; payload: string | undefined };

function exhaustive({ kind, payload }: Action): string {
  switch (kind) {
    case "A":
      return payload.toFixed(0);
    case "B":
      return payload.toUpperCase();
    default: {
      const none: never = payload;
      return none;
    }
  }
}

function guarded(action: Maybe): string {
  if (action.payload) {
    const { kind, payload } = action;
    if (kind === "A") {
      return payload.toFixed(1);
    }
  }
  return "none";
}

function tuple(x: [true, number] | [false, string]): string {
  const [ok, value] = x;
  return ok ? value.toFixed(0) : value.toUpperCase();
}

function arity([x, y]: [1, 2] | [3, 4] | [5]): string {
  if (x === 5) {
    const missing: undefined = y;
    return String(missing);
  }
  if (y === undefined) {
    const five: 5 = x;
    return String(five);
  }
  return String(x + y);
}

function main(): void {
  assert(exhaustive({ kind: "A", payload: 2 }) === "2", "switch case");
  assert(guarded({ kind: "A", payload: 2 }) === "2.0", "guarded source");
  assert(tuple([true, 3]) === "3" && tuple([false, "s"]) === "S", "tuple elements");
  assert(arity([5]) === "undefined" && arity([1, 2]) === "3", "tuples of different lengths");
  let [key, value]: [string, string?] = ["k"];
  const absent: undefined = value;
  assert(key === "k" && absent === undefined, "a missing optional element");
}
