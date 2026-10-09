interface Hook { run?(value: number): number; }
class Holder { run?(value: number): number; }
function invoke(hook: Hook): number | undefined { return hook.run?.(3); }
function main(): void {
  const empty: Hook = {};
  const provided: Hook = { run(value: number): number { return value + 1; } };
  assert(invoke(empty) === undefined);
  assert(invoke(provided) === 4);
  const holder = new Holder();
  assert(holder.run?.(1) === undefined);
  holder.run = (value: number): number => value * 2;
  assert(holder.run?.(3) === 6);
}
