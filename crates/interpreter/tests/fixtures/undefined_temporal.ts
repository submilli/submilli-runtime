function main(): void {
  const date = Temporal.PlainDate.from("2024-03-15");
  const midnight = date.toPlainDateTime(undefined);
  assert(midnight.hour === 0 && midnight.day === 15, "undefined time defaults to midnight");

  const zoned = date.toZonedDateTime({ timeZone: "UTC", plainTime: undefined });
  assert(zoned.hour === 0 && zoned.timeZoneId === "UTC", "undefined plainTime uses midnight");

  const changed = date.with({ year: undefined, month: undefined, day: 20 });
  assert(changed.year === 2024 && changed.month === 3 && changed.day === 20,
    "undefined numeric fields preserve existing values");

  const instant = Temporal.Instant.from("2024-03-15T00:00:00Z");
  const later = Temporal.Instant.from("2024-03-15T00:00:05Z");
  const difference = instant.until(later, {
    smallestUnit: undefined,
    largestUnit: undefined,
    roundingMode: undefined,
    roundingIncrement: undefined,
  });
  assert(difference.seconds === 5, "undefined rounding fields use defaults");
  assert(instant.until(later, undefined).seconds === 5, "undefined options use defaults");
}
