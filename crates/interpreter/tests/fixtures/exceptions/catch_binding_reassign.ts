// A `catch` binding is a mutable local, as in TypeScript. A closure that
// captures and reassigns it writes the same binding the clause reads.
class AppError extends Error {
  code: number = 7;
}

function replaced(): string {
  try {
    throw new Error("boom");
  } catch (e) {
    e = new Error("replaced");
    return e.message;
  }
}

function replacedByClosure(): string {
  try {
    throw new Error("first");
  } catch (e) {
    const replace = (): void => {
      e = new Error("second");
    };
    replace();
    return e.message;
  }
}

function filtered(): string {
  try {
    throw new AppError("a");
  } catch (e: AppError) {
    e = new AppError("b");
    return `${e.message}${e.code}`;
  }
}

function narrowedInClosure(): number[] {
  try {
    throw new AppError("c");
  } catch (e) {
    if (e instanceof AppError) {
      return [1, 2].map(() => e.code);
    }
    return [];
  }
}

function main(): void {
  assert(replaced() === "replaced", "a direct reassignment sticks");
  assert(replacedByClosure() === "second", "a closure writes the binding the clause reads");
  assert(filtered() === "b7", "a filtered clause's binding is reassignable");
  assert(narrowedInClosure().join(",") === "7,7", "an unassigned binding stays narrowed");
}
