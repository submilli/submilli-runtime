function fmt(completedAt: string | null): string {
  return `Completed: ${completedAt!}`;
}

function main(): void {
  assert(fmt("2026-01-01") === "Completed: 2026-01-01", "non-null value passes through");

  let caught = "";
  try {
    fmt(null);
  } catch (e: Error) {
    caught = e.name;
  }
  assert(caught === "TypeError", "asserting on null throws a catchable TypeError");
}
