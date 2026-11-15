import session from "submilli:session";
import { get } from "submilli:session";

function load<T>(key: string): T | undefined {
  return get<T>(key);
}

function main(): void {
  session.set("instant", "2026-10-15T06:00:00Z");
  const instant = session.get<Temporal.Instant>("instant")!;
  assert(instant.toString() === "2026-10-15T06:00:00Z", "direct decoder");

  const read = get;
  const forwarded = load<Temporal.Instant>("instant");
  assert(forwarded !== undefined && forwarded.equals(instant), "forwarded decoder");
  const concrete: (key: string) => Temporal.Instant | undefined = get;
  const bound = concrete("instant");
  assert(bound !== undefined && bound.equals(instant), "bound decoder");
  assert(read<Temporal.Instant>("missing") === undefined, "missing skips decoder");

  session.set("duration", { hours: 1, minutes: 30 });
  const duration = get<Temporal.Duration>("duration");
  assert(duration !== undefined && duration.toString() === "PT1H30M", "object input");

  session.set("bad", "not-an-instant");
  let rejected = false;
  try {
    get<Temporal.Instant>("bad");
  } catch (e) {
    rejected = e instanceof RangeError;
  }
  assert(rejected, "decoder error is catchable");

  session.set("null", null);
  rejected = false;
  try {
    get<Temporal.Instant>("null");
  } catch (e) {
    rejected = e instanceof TypeError;
  }
  assert(rejected, "stored null reaches decoder");
  assert(get<string>("instant") === "2026-10-15T06:00:00Z", "decode does not mutate storage");
}
