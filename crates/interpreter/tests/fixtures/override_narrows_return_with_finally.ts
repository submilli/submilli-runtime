// An override that narrows its base's return type returns through the vtable
// slot the base recorded, not through its own declared type. A `finally` on
// the way out stashes the value in a local, which has to be typed from that
// recorded slot: the base's `string | number` slot and the override's `string`
// lower differently, and a stash typed from either language type is the wrong
// width for the other.

const log: string[] = [];

class Base {
  pick(n: number): string | number {
    return n;
  }
  maybe(n: number): string | null {
    return n > 0 ? "pos" : null;
  }
}

class Kid extends Base {
  pick(n: number): string {
    try {
      return "kid";
    } finally {
      log.push("pick");
    }
  }
  maybe(n: number): string {
    try {
      return "always";
    } finally {
      log.push("maybe");
    }
  }
}

class UnknownBase {
  pick(n: number): unknown {
    return n;
  }
}

// The near miss of the primitive case: the base's slot is the erased nullable
// `$Object`, so narrowing all the way to `number` is fine here.
class FromUnknown extends UnknownBase {
  pick(n: number): number {
    try {
      return 42;
    } finally {
      log.push("unknown");
    }
  }
}

class Grandkid extends Kid {
  pick(n: number): string {
    try {
      return "grandkid";
    } finally {
      log.push("grandkid");
    }
  }
}

class FinallyReturns extends Base {
  pick(n: number): string {
    try {
      return "try";
    } finally {
      log.push("finally-return");
      return "finally";
    }
  }
}

class CaughtThenReturns extends Base {
  pick(n: number): string {
    try {
      throw new Error("x");
    } catch (e) {
      try {
        return "caught";
      } finally {
        log.push("inner");
      }
    } finally {
      log.push("outer");
    }
  }
}

function main(): void {
  const b: Base = new Kid();
  assert(b.pick(1) === "kid", "narrowed override with finally");
  assert(b.maybe(1) === "always", "nullable base slot narrowed to non-null");

  const g: Base = new Grandkid();
  assert(g.pick(1) === "grandkid", "two-level narrowed override with finally");

  // The unnarrowed base body still returns through the same slot.
  const plain: Base = new Base();
  assert(plain.pick(3) === 3, "base returns the number arm");
  assert(plain.maybe(-1) === null, "base returns the null arm");

  const u: UnknownBase = new FromUnknown();
  assert(u.pick(1) === 42, "narrowed override off an erased base slot");

  // The `finally`'s own return wins over the `try`'s.
  const f: Base = new FinallyReturns();
  assert(f.pick(1) === "finally", "the finally's own return wins");

  // A return through a `finally` nested inside a `catch`.
  const c: Base = new CaughtThenReturns();
  assert(c.pick(1) === "caught", "return through a finally inside a catch");

  assert(log.length === 7, "every finally ran");
}
