// expect-error: expected `string | Temporal.Duration | Temporal.DurationFields`, got `number`
function main(): void {
  const d = Temporal.Duration.from({ hours: 1 });
  d.add(5);
}
