function main(): void {
  let result: string;
  try { result = "ok"; } catch { return; }
  assert(result === "ok", "returning catch leaves the assigned path");

  let total = 0;
  for (const text of ["2026-11-03", "invalid", "2026-11-04"]) {
    let date: Temporal.PlainDate;
    try { date = Temporal.PlainDate.from(text); } catch { continue; }
    total += date.day;
  }
  assert(total === 7, "continuing catch skips unassigned reads");

  let count: number;
  try { count = 4; } catch { count = 5; }
  assert(count === 4, "both paths assign");
}
