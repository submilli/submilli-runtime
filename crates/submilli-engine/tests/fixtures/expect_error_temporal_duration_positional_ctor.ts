// expect-error: expected `Temporal.DurationFields`, got `number`
// expect-error: years?: number
function main(): void {
  new Temporal.Duration(1);
}
