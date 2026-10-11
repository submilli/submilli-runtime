interface Logger {
  log(msg: string): void;
}
function main(): void {
  let calls = 0;
  const present: Logger = { log(msg: string): void { calls = calls + 1; } };
  const absent: Logger | null = null as Logger | null;
  const annotated: void | null = absent?.log("skipped");
  const inferred = absent?.log("skipped");
  assert(annotated === undefined);
  assert(inferred === undefined);
  assert(calls === 0);
  const completed = present?.log("called");
  assert(completed === undefined);
  assert(calls === 1);
}
