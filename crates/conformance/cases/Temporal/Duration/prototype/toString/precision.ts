// test262: test/built-ins/Temporal/Duration/prototype/toString/precision.js

function main(): void {
  const durationString = "PT0.084000159S";
  const duration = Temporal.Duration.from(durationString);
  const precisionString = duration.toString({ smallestUnit: "milliseconds" });

  assertSameValue(durationString, duration.toString());
  assertSameValue(precisionString, "PT0.084S");
}
