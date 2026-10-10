interface Opts {
  unit: string;
}

class Formatter {
  // The slot's recorded parameter type is `(ref null $ObjectShape)` — nullable,
  // but narrower than the `(ref null $Object)` an interface-typed value lowers to.
  format(opts: { unit: string } | null): string {
    return opts === null ? "none" : opts.unit;
  }
}

class Loud extends Formatter {
  format(opts: { unit: string } | null): string {
    return opts === null ? "none!" : opts.unit + "!";
  }
}

// Dispatches through the vtable slot, so the argument is coerced into the
// recorded slot type rather than the declared parameter type.
function shout(f: Formatter, opts: Opts): string {
  return f.format(opts);
}

function main(): void {
  const opts: Opts = { unit: "day" };
  assert(shout(new Formatter(), opts) === "day", "interface value into an object slot");
  assert(shout(new Loud(), opts) === "day!", "override slot takes the same coercion");
  assert(new Formatter().format(null) === "none", "null still reaches the slot");
}
