function main(): void {
  const obj: Record<string, number> = { x: 2, y: 3 };
  let byKey = 0;
  for (const k in obj) byKey += obj[k];
  let byEntry = 0;
  for (const [k, v] of Object.entries(obj)) byEntry += obj[k] + v;
  assert(byKey === 5 && byEntry === 10, "both suggested rewrites work");

  let runs = 0;
  for (let [present = 'x' in obj] = []; present; present = false) runs++;
  for (let { present = 'y' in obj } = {}; present; present = false) runs++;
  for (let [less = 1 < 2] = []; less; less = false) runs++;
  assert(runs === 3, "in and comparisons inside defaults do not change the loop kind");
  console.log(byKey, byEntry, runs);
}
