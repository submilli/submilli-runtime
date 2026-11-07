function main(): void {
  const date = Temporal.PlainDate.from("2023-05-18");
  const dt = Temporal.PlainDateTime.from("2023-05-18T12:30");
  const z = Temporal.ZonedDateTime.from("2023-05-18T12:30Z[UTC]");
  assert(date.toPlainDateTime("12:30").equals(dt), "date plus string time");
  assert(date.toPlainDateTime({ hour: 12, minute: 30 }).equals(dt), "date plus bag time");
  assert(date.toPlainDateTime(z).equals(dt), "date plus zoned time");
  assert(date.toZonedDateTime(z).hour === 0, "zoned value as time zone");
  assert(date.toZonedDateTime({ timeZone: z, plainTime: "12:30" }).equals(z), "zone and time Like options");
  assert(dt.toZonedDateTime(z).equals(z), "datetime with zone Like");
  assert(z.withTimeZone(z).equals(z), "withTimeZone Like");
  assert(z.toInstant().toZonedDateTimeISO(z).equals(z), "instant zone Like");
  assert(Temporal.ZonedDateTime.from({ year: 2023, month: 5, day: 18, hour: 12, minute: 30, timeZone: z }).equals(z), "zone Like in bag");
  assert(Temporal.Duration.from("PT1H").add("PT2H").hours === 3, "duration string add");
  assert(Temporal.Duration.from("PT3H").subtract("PT2H").hours === 1, "duration string subtract");
  assert(Temporal.Duration.compare("P1M", "P28D", { relativeTo: "2023-02-01" }) === 0, "string relativeTo");
  assert(Temporal.Duration.compare("P1M", "P28D", { relativeTo: { year: 2023, month: 2, day: 1 } }) === 0, "bag relativeTo");
  assert(Temporal.Duration.compare("PT1H", "PT60M", { relativeTo: dt }) === 0, "datetime relativeTo");
  assert(Temporal.Duration.compare("P1D", "PT23H", { relativeTo: "2024-03-10T00:00-05:00[America/New_York]" }) === 0, "zoned string relativeTo across DST");
}
