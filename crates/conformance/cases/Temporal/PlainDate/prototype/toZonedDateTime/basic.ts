// test262: test/built-ins/Temporal/PlainDate/prototype/toZonedDateTime/basic.js
// The { plainTime: "12:00" } string-property variant is dropped: the options
// bag types plainTime as Temporal.PlainTime, so the string form is a compile
// error here (same intent as the PlainTime-object row that is kept).

function main(): void {
  const plainDate = Temporal.PlainDate.from("2020-01-01");
  const timeZone = "UTC";
  const plainTime = Temporal.PlainTime.from("12:00");

  let result = plainDate.toZonedDateTime({ timeZone, plainTime });
  assertSameValue(result.toString(), "2020-01-01T12:00:00+00:00[UTC]", "objects passed");

  result = plainDate.toZonedDateTime(timeZone);
  assertSameValue(result.toString(), "2020-01-01T00:00:00+00:00[UTC]", "time zone string argument");

  result = plainDate.toZonedDateTime({ timeZone, plainTime });
  assertSameValue(result.toString(), "2020-01-01T12:00:00+00:00[UTC]", "time zone string property");
}
