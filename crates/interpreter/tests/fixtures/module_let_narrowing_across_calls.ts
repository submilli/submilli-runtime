// A narrowing on a module-level `let` ends only at a call that may run a
// function assigning it: a call of a built-in that is handed no function keeps
// it, and so does any call when no function assigns the variable.
let label: string | null = "abc";
let fixed: string | null = "xy";

function clear(): void {
  label = null;
}

function describe(): string {
  return "described";
}

function lengthAcrossBuiltins(): number {
  if (label !== null) {
    console.log(Math.floor(1.5), "x".toUpperCase(), [3, 1].join(","));
    return label.length;
  }
  return -1;
}

function unwrittenAcrossUserCall(): number {
  if (fixed !== null) {
    describe();
    clear();
    return fixed.length;
  }
  return -1;
}

function renarrowAfterCall(): number {
  if (label !== null) {
    clear();
    if (label !== null) {
      return label.length;
    }
  }
  return -1;
}

function main(): void {
  assert(lengthAcrossBuiltins() === 3, "built-in calls keep the narrowing");
  assert(unwrittenAcrossUserCall() === 2, "no function writes `fixed`");
  assert(renarrowAfterCall() === -1, "the call cleared `label`");
  console.log("ok");
}
