function choose(flag: boolean): string {
  let value: string;
  if (flag) { value = "yes"; } else { value = "no"; }
  return value;
}

function main(): void {
  assert(choose(true) === "yes", "then branch");
  assert(choose(false) === "no", "else branch");
  let count: number;
  count = 3;
  count++;
  count += 2;
  assert(count === 6, "numeric storage and updates");
  let flag: boolean;
  flag = true;
  assert(flag, "boolean storage");
  let obj: { value: string };
  obj = { value: "ok" };
  assert(obj.value === "ok", "object storage");
  let final: string;
  try { console.log("try"); } finally { final = "done"; }
  assert(final === "done", "finally assignment");
  let loop: number;
  while (true) { loop = 7; break; }
  assert(loop === 7, "break assignment");
  let captured: string;
  captured = "captured";
  const read = (): string => captured;
  assert(read() === "captured", "capture after assignment");
  let unused: number;
}
