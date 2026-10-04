function main(): void {
  const eastern = Temporal.ZonedDateTime.from("2024-03-09T12:00:00-05:00[US/Eastern]");
  const newYork = eastern.withTimeZone("America/New_York");
  assert(eastern.equals(newYork), "named aliases compare equal");
  assert(Object.is(eastern, newYork), "structural equality uses the same aliases");
  assert(eastern === newYork, "direct equality uses the same aliases");
  assert(!eastern.equals(newYork.add({seconds: 1})), "instant still matters");

  const utc = eastern.withTimeZone("UTC");
  assert(utc.equals(eastern.withTimeZone("GMT")), "GMT is a named UTC alias");
  assert(!utc.equals(eastern.withTimeZone("+00:00")), "named and offset zones differ");
  assert(eastern.withTimeZone("+01:00").equals(eastern.withTimeZone("+0100")), "offset aliases");

  const iceland = eastern.withTimeZone("Iceland");
  assert(iceland.equals(eastern.withTimeZone("Atlantic/Reykjavik")), "country-specific alias");
  assert(!iceland.equals(eastern.withTimeZone("Africa/Abidjan")), "equal rules do not imply identity");
  assert(eastern.withTimeZone("Pacific/Truk").equals(eastern.withTimeZone("Pacific/Chuuk")), "historical alias");
}
