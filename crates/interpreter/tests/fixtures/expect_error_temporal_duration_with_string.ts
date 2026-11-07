// expect-error: expected `Temporal.Duration | Temporal.DurationFields`, got `string`
function main(): void {
  Temporal.Duration.from("PT1H").with("PT2H");
}
