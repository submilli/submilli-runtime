function noop(value: number = 1): void {}
function main(): void {
  const value: unknown = noop;
  let caught = false;
  let castCompleted = false;
  try { const fn = value as () => number; castCompleted = true; fn(); }
  catch (e: Error) { caught = true; }
  assert(!castCompleted, "reject at the cast before invocation");
  assert(caught, "a checked cast still validates the return convention");
}
