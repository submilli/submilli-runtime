// Testing a function, which is always defined, where a call was meant is a
// mistake, as tsc reports (TS2774), unless the code the test guards uses it.
// So is testing an enum member, whose value is fixed (TS2845).
// expect-error: this condition will always return true since this function is always defined
// expect-error: this condition will always return `false`
// expect-error-count: 5
enum Level {
  Off,
  On,
}

interface Handlers {
  ready: () => boolean;
  done?: () => void;
}

function isReady(): boolean {
  return true;
}

function main(): void {
  const handlers: Handlers = { ready: isReady };
  if (isReady) {}
  const label = handlers.ready ? "ready" : "waiting";
  const both = isReady && 1;
  if (isReady && label) {}
  if (Level.Off) {}
  if (isReady) {
    isReady();
  }
  if (handlers.ready) {
    handlers.ready();
  }
  const called = isReady && isReady();
  if (handlers.done) {}
  if (isReady()) {}
  console.log(label, both, called);
}
