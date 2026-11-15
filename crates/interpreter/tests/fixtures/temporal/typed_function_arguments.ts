function run(at: Temporal.Instant): string {
  return at.toString();
}

function nested(input: { at: Temporal.Instant; limit: number }): string {
  return `${input.at.toString()}|${input.limit}`;
}

function main(): void {
  const at = Temporal.Instant.from("2026-10-15T09:00:00+03:00[Asia/Jerusalem]");
  assert(run(at) === "2026-10-15T06:00:00Z", "typed Temporal argument");
  assert(nested({ at, limit: 3 }) === "2026-10-15T06:00:00Z|3", "nested typed Temporal argument");
}
