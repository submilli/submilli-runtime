function main(): void {
  const opts: Temporal.SinceUntilOptions = { largestUnit: "day" };
  const d = Temporal.PlainDate.from("2020-01-02");
  const later = Temporal.PlainDate.from("2020-03-02");
  assert(
    d.until(later, opts).toString() === "P60D",
    "until with an interface-typed options value",
  );
  assert(
    later.since(d, opts).toString() === "P60D",
    "since with an interface-typed options value",
  );
}
