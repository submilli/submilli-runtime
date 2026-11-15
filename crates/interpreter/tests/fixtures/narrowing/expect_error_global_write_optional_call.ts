// expect-error: cannot read field `length`
let shared: string | number = "a";
function mutate(): void { shared = 5; }
function run(callback: (() => void) | undefined): number {
  if (typeof shared === "string") {
    callback?.();
    return shared.length;
  }
  return 0;
}
function main(): number { return run(mutate); }
