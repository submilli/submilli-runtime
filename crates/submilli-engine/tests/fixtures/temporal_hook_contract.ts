function main(): void {
  const zoned = Temporal.ZonedDateTime.from("2024-03-09T12:00:00Z[UTC]");
  assert(zoned.toJSON() === zoned.toString(), "toJSON returns the unquoted ISO value");
  assert(JSON.stringify(zoned) === JSON.stringify(zoned.toJSON()), "JSON hook serializes the same ISO value");
  const boxed: unknown = zoned;
  assert(JSON.stringify(boxed) === JSON.stringify(zoned.toJSON()), "erased JSON hook serializes the same ISO value");
  const eastern = Temporal.ZonedDateTime.from("2024-01-01T00:00Z[US/Eastern]");
  const primary = Temporal.ZonedDateTime.from("2024-01-01T00:00Z[America/New_York]");
  const left: unknown = eastern;
  const right: unknown = primary;
  assert(eastern.equals(primary), "direct equality canonicalizes aliases");
  assert(Object.is(left, right), "hook equality canonicalizes the same aliases");
}
