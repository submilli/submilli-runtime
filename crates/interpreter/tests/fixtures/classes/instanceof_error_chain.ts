// `instanceof` along a depth-2 `extends` chain rooted at the host-owned
// `Error`: true along the real chain, false downward, and the walk reaches the
// host `Error` singleton at the root.
class AppError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "AppError";
  }
}

class TimeoutError extends AppError {
  constructor() {
    super("timed out");
    this.name = "TimeoutError";
  }
}

function main(): void {
  const t: Error = new TimeoutError();
  assert(t instanceof TimeoutError);
  assert(t instanceof AppError);
  assert(t instanceof Error);

  const base: Error = new Error("plain");
  assert(base instanceof Error);
  assert(!(base instanceof AppError));
  assert(!(base instanceof TimeoutError));

  const mid: Error = new AppError("mid");
  assert(mid instanceof AppError);
  assert(mid instanceof Error);
  assert(!(mid instanceof TimeoutError));
}
