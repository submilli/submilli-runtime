class StorageFullError extends QuotaExceededError {
  constructor(message: string) {
    super(message);
  }
}

function main(): void {
  const error = new QuotaExceededError("budget spent");
  assert(error instanceof QuotaExceededError, "quota identity");
  assert(error instanceof Error, "base identity");
  assert(!((error as unknown) instanceof RangeError), "distinct from argument errors");
  assert(error.name === "QuotaExceededError", "quota name");
  assert(error.message === "budget spent", "quota message");
  assert(error.toString() === "QuotaExceededError: budget spent", "quota display");
  const child = new StorageFullError("full");
  assert(child instanceof QuotaExceededError, "subclass quota identity");
  assert(child instanceof Error, "subclass base identity");
  assert(child.name === "QuotaExceededError", "subclass initialized by super");
  let caught = false;
  try {
    throw error;
  } catch (e) {
    caught = e instanceof QuotaExceededError && e instanceof Error && !((e as unknown) instanceof RangeError);
  }
  assert(caught, "catch preserves identity");
  let range = false;
  try {
    "x".repeat(-1);
  } catch (e) {
    range = e instanceof RangeError && !((e as unknown) instanceof QuotaExceededError);
  }
  assert(range, "argument cap remains RangeError");
}
