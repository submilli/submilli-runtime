// The return-position counterpart of `interface_value_into_object_slot`. An
// interface-typed value lowers to `(ref null $Object)`; a `{…} | null` return
// slot is the narrower `(ref null $ObjectShape)`, so the coercion needs a real
// downcast. A union target is the shape that used to slip past the check,
// since it is a `Type::Union` rather than a `Type::Object`.
interface Named {
  name: string;
  size: number;
}

function fromFunction(n: Named, keep: boolean): { name: string; size: number } | null {
  if (!keep) {
    return null;
  }
  return n;
}

class Wrap {
  fromMethod(n: Named, keep: boolean): { name: string; size: number } | null {
    if (!keep) {
      return null;
    }
    return n;
  }
}

function intoArgument(o: { name: string; size: number } | null): string {
  return o === null ? "none" : o.name;
}

function main(): void {
  const v: Named = { name: "a", size: 1 };

  const got = fromFunction(v, true);
  assert(got !== null && got.name === "a", "interface into a nullable structural return");
  assert(fromFunction(v, false) === null, "null still reaches the same slot");

  const m = new Wrap().fromMethod(v, true);
  assert(m !== null && m.size === 1, "same coercion from a method");

  assert(intoArgument(v) === "a", "interface into a nullable structural parameter");

  const local: { name: string; size: number } | null = v;
  assert(local !== null && local.name === "a", "interface into a nullable structural local");

  const elems: ({ name: string; size: number } | null)[] = [v, null];
  const first = elems[0];
  assert(first !== null && first.size === 1, "interface into a nullable structural array element");

  const closure = (keep: boolean): { name: string; size: number } | null => (keep ? v : null);
  const c = closure(true);
  assert(c !== null && c.name === "a", "interface into a nullable structural closure return");
}
