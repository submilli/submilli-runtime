class ApprovalTimeoutError extends TimeoutError {
  constructor(message: string) {
    super(message);
  }
}

function main(): void {
  const error = new TimeoutError("no answer");
  assert(error instanceof TimeoutError, "timeout identity");
  assert(error instanceof Error, "base identity");
  assert(!((error as unknown) instanceof QuotaExceededError), "distinct from quota errors");
  assert(error.name === "TimeoutError", "timeout name");
  assert(error.message === "no answer", "timeout message");
  assert(error.toString() === "TimeoutError: no answer", "timeout display");
  const child = new ApprovalTimeoutError("nobody approved");
  assert(child instanceof TimeoutError, "subclass timeout identity");
  assert(child instanceof Error, "subclass base identity");
  assert(child.name === "TimeoutError", "subclass initialized by super");
  let caught = false;
  try {
    throw error;
  } catch (e) {
    caught = e instanceof TimeoutError && e instanceof Error && !((e as unknown) instanceof RangeError);
  }
  assert(caught, "catch preserves identity");
}
