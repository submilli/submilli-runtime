// expect-error: is not yet supported
function main(): void {
  const instant = JSON.parse('"2026-10-15T06:00:00Z"') as Temporal.Instant;
  console.log(instant.toString());
}
