function orElse<T>(value: T | null | undefined, fallback: T): T {
  if (value !== null && value !== undefined) {
    return value;
  }
  return fallback;
}

function firstTruthy<T, U>(first: T, second: U): T | U {
  return first || second;
}

function isPresent<T>(value: T): boolean {
  if (value !== undefined && value !== null) {
    const present: {} = value;
    return present !== undefined;
  }
  return false;
}

function keep<T>(x: T): T {
  return x;
}

function passOn<T>(value: T): T {
  if (value) {
    return keep(value);
  }
  if (value !== null) {
    const wrapped = [value];
    return wrapped[0];
  }
  return value;
}

function andPresent<T>(value: T): string {
  if (value !== null && value !== undefined) {
    return String(value && "and");
  }
  return "absent";
}

function lastPresent<T>(items: T[]): T | null {
  let best = items[0] || null;
  for (const item of items) {
    if (item) {
      best = item;
    }
  }
  return best;
}

function lastDefined<T>(items: T[]): T | null {
  let best = items[0] || null;
  for (const item of items) {
    if (item !== undefined) {
      best = item;
    }
  }
  return best;
}

function regroup<T>(value: T): number {
  let either = value !== undefined ? value : null;
  if (value) {
    either = value;
  }
  const defined = value !== undefined ? value : null;
  const nonNull = value !== null ? value : undefined;
  return [either, defined, nonNull].length;
}

function definedOnly<T>(value: T, other: T): T[] {
  if (value !== undefined && other !== null) {
    let copy = value;
    copy = keep(value);
    const box = { v: value };
    box.v = value;
    const both = [value, other];
    both.push(copy);
    return both;
  }
  return [];
}

function orMaybe<T>(value: T): {} | null | undefined {
  return value;
}

function collect<T>(values: T[]): ({} | null | undefined)[] {
  const out: ({} | null | undefined)[] = [];
  for (const value of values) {
    out.push(value);
  }
  return out;
}

function maybe<V>(value?: V): V | undefined {
  return value;
}

function orNull<V>(value: V | null): V | null {
  return value;
}

function required<V>(value: V | undefined): V {
  if (value === undefined) {
    throw new Error("missing");
  }
  return value;
}

function either<V, W>(first: V | W, second: W): W {
  return second;
}

function orText<V>(value: V | string | null | undefined): string {
  return typeof value === "string" ? value : "other";
}

function presentOr<T>(value: T, flag: boolean): string {
  if (value !== undefined) {
    const picked = flag ? value : null;
    if (picked !== null) {
      const present: {} = picked;
      return String(present);
    }
    const fallback: {} = orNull(value) ?? "none";
    return String(fallback);
  }
  return "absent";
}

function throughOptionals<T>(value: T): string {
  if (value !== undefined) {
    const a: T | undefined = maybe(value);
    const b: T | null = orNull(value);
    const c: T = required(value);
    const d: number = either(value, 1);
    return String(a) + String(b) + String(c) + String(d) + orText(value);
  }
  return "absent";
}

function main(): void {
  assert(orElse<number>(null, 3) === 3, "`null` falls back");
  assert(orElse(0, 3) === 0, "a falsy value that is present is kept");
  assert(orElse<string>(undefined, "c") === "c", "`undefined` falls back");
  assert(firstTruthy(0, "x") === "x", "`||` skips a falsy `T`");
  assert(firstTruthy("y", 2) === "y", "`||` keeps a truthy `T`");
  assert(isPresent(0) && !isPresent(null) && !isPresent(undefined), "`T & {}` holds only present values");
  assert(passOn(2) === 2 && passOn(0) === 0 && passOn(null) === null, "a narrowed `T` passes on as a `T`");
  assert(andPresent(0) === "0" && andPresent(1) === "and", "`&&` keeps a falsy present `T`");
  assert(passOn<number | undefined>(undefined) === undefined, "an absent `T` passes on");
  assert(lastPresent([1, 0, 2, 0]) === 2, "a narrowed `T` fits a `T | null` variable");
  assert(orMaybe(0) === 0, "a `T` fits `{} | null | undefined`");
  assert(lastDefined([1, 2, undefined]) === 2, "one narrowing of `T` fits a union holding another");
  assert(regroup(0) === 3, "narrowings of `T` regroup into unions");
  assert(definedOnly(1, 2).length === 3, "a one-sided narrowing of `T` passes on and joins another");
  assert(JSON.stringify(collect<number | string | null>([1, null, "a"])) === "[1,null,\"a\"]", "a `T` element fits `{} | null | undefined`");
  assert(throughOptionals<number | null>(null) === "nullnullnull1other", "a narrowed `T` binds a `V | null` parameter as itself");
  assert(presentOr(0, true) === "0" && presentOr<number | null>(null, false) === "none", "a union holding a narrowed `T` narrows again");
  assert(throughOptionals<number | undefined>(undefined) === "absent", "an absent `T` skips the calls");
}
