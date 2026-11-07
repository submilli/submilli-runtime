function rejects(offset: string): boolean {
  try {
    Temporal.ZonedDateTime.from({ year: 2023, month: 5, day: 18, timeZone: "UTC", offset });
    return false;
  } catch (e) { return e instanceof RangeError; }
}
function main(): void {
  for (const offset of ["+00", "+0000", "+00:00", "+000000", "+00:00:00", "+00:00:00.0", "+000000,0", "−00:00", "+00:00:00.000000000"]) {
    assert(Temporal.ZonedDateTime.from({ year: 2023, month: 5, day: 18, timeZone: "UTC", offset }).hour === 0, offset);
  }
  for (const offset of ["UTC", "Etc/Unknown", "Z", "+24:00", "+00:60", "+00:00:60", "+00:00.0", "+00:00:00.", "+00:00:00.0000000001", "+00:00:00.1", " +00:00", "+0:00"]) {
    assert(rejects(offset), offset);
  }
  const later = Temporal.ZonedDateTime.from({ year: 2024, month: 11, day: 3, hour: 1, minute: 30, timeZone: "America/New_York", offset: "-05:00" });
  assert(later.offset === "-05:00", "offset resolves overlap");
  assert(Temporal.ZonedDateTime.from({ year: 2024, month: 3, day: 10, hour: 2, minute: 30, timeZone: "America/New_York" }).hour === 3, "compatible gap resolution");
}
