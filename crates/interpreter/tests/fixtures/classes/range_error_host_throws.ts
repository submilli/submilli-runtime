// Host-thrown range failures surface as the built-in RangeError subclass, so
// `catch (e: RangeError)` filters them and `instanceof` narrows.
function main(): void {
  // BigInt division by zero.
  let div = "";
  try {
    const z: bigint = 0n;
    const r: bigint = 5n / z;
    div = r.toString();
  } catch (e: RangeError) {
    div = e.name + ":" + e.message;
  }
  assert(div === "RangeError:Division by zero", div);

  // Negative String.repeat count.
  let rep = "";
  const n: number = -1;
  try {
    rep = "x".repeat(n);
  } catch (e: RangeError) {
    rep = e.message;
  }
  assert(rep === "Invalid count value", rep);

  // String.normalize with an invalid form.
  let norm = "";
  try {
    norm = "x".normalize("BAD");
  } catch (e: RangeError) {
    norm = "range";
  }
  assert(norm === "range", "normalize invalid form is RangeError");

  // bigint.pow negative exponent.
  let pow = "";
  try {
    const neg: bigint = -1n;
    const r: bigint = 2n ** neg;
    pow = r.toString();
  } catch (e: RangeError) {
    pow = "range";
  }
  assert(pow === "range", "negative bigint exponent is RangeError");

  // Temporal: calendar units are invalid on a bare Instant.
  const a: Temporal.Instant = Temporal.Instant.from("2024-03-09T15:30:45Z");
  let sub = "";
  try {
    const w: Temporal.Instant = a.subtract({ weeks: 1 });
    sub = w.toString();
  } catch (e: RangeError) {
    sub = "range";
  }
  assert(sub === "range", "Instant.subtract calendar unit is RangeError");

  // Temporal: invalid ISO string.
  let iso = "";
  try {
    const bad: Temporal.Instant = Temporal.Instant.from("not-a-timestamp");
    iso = bad.toString();
  } catch (e: RangeError) {
    iso = "range";
  }
  assert(iso === "range", "invalid ISO string is RangeError");

  // Temporal: unknown time zone.
  let tz = "";
  try {
    const z: Temporal.ZonedDateTime = a.toZonedDateTimeISO("Mars/Olympus");
    tz = z.toString();
  } catch (e: RangeError) {
    tz = "range";
  }
  assert(tz === "range", "unknown time zone is RangeError");

  // A caught host RangeError narrows with instanceof from a catch-all arm,
  // and Error.isError accepts it.
  let kind = "";
  try {
    const z: bigint = 0n;
    const r: bigint = 5n % z;
    kind = r.toString();
  } catch (e) {
    assert(Error.isError(e), "host RangeError is an Error");
    if (e instanceof RangeError) {
      kind = "range";
    } else {
      kind = "base";
    }
  }
  assert(kind === "range", "instanceof narrows a host-thrown RangeError");

  // A user subclass of RangeError keeps the chain: it is a RangeError and an
  // Error, and the typed arm binds it.
  let sub2 = "";
  try {
    throw new NegativeAmount("amount must be non-negative", -5);
  } catch (e: RangeError) {
    sub2 = e.name;
    if (e instanceof NegativeAmount) {
      sub2 = sub2 + ":" + e.amount.toString();
    }
  }
  assert(sub2 === "NegativeAmount:-5", sub2);
}

class NegativeAmount extends RangeError {
  amount: number;
  constructor(message: string, amount: number) {
    super(message);
    this.name = "NegativeAmount";
    this.amount = amount;
  }
}
