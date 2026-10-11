const seed: number = 4;
function fromGlobal(value = seed): number { return value; }
function fromLater(value = later()): number { return value; }
function later(): number { return 5; }
function main(): void {
  assert(fromGlobal() === 4, "default parameter type inferred from global declaration");
  assert(fromLater() === 5, "default parameter type inferred from later function return");
}
