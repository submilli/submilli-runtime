let trace = "";

function numericReturn(): number {
  try { return 7; }
  finally {
    for (let i = 0; i < 3; i++) {
      try {
        if (i === 0) { continue; }
        if (i === 1) { break; }
      } finally { trace = trace + "i"; }
    }
  }
}

function referenceReturn(): string {
  try { return "first"; }
  finally {
    try { throw new Error("e"); }
    catch (e) { trace = trace + e.message; }
    finally { trace = trace + "z"; }
  }
}

function continueOverridesThrow(): void {
  for (let i = 0; i < 3; i++) {
    try {
      try { throw new Error("e"); }
      finally { continue; }
    } finally { trace = trace + "o"; }
  }
}

function main(): void {
  assert(numericReturn() === 7);
  assert(trace === "ii");
  trace = "";
  assert(referenceReturn() === "first");
  assert(trace === "ez");
  trace = "";
  continueOverridesThrow();
  assert(trace === "ooo");
}
