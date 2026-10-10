// A `void` type argument at a covariant parameter falls back to comparing
// members, as in tsc: a task returning a number serves as one returning
// nothing, since a `void` return accepts any value.
interface Task<T> {
  run(): T;
}

function runAll(tasks: Task<void>[]): number {
  let count = 0;
  for (const task of tasks) {
    task.run();
    count += 1;
  }
  return count;
}

function main(): void {
  const numbered: Task<number> = { run: () => 1 };
  const quiet: Task<void> = numbered;
  quiet.run();
  assert(runAll([numbered, quiet]) === 2, "both tasks run");
}
