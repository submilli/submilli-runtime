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

function orMaybe<T>(value: T): {} | null | undefined {
  return value;
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
}
