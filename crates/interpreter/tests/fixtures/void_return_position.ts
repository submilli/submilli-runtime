// The shapes that *are* legal: returning a `void` call from a `void` function,
// and an alias for `void` in return position.
type V = void;

function sideEffect(): void {}

function passthrough(): void {
  return sideEffect();
}

function aliased(): V {
  return sideEffect();
}

// A type parameter the declaration only ever puts in return position may be
// instantiated at `void` — a `void` return has no result slot at all. This is
// the one place `void` is legitimate outside a function's own return type.
interface Sink<T> {
  emit(x: string): T;
}

function drain(s: Sink<void>): void {
  s.emit("a");
}

function main(): void {
  passthrough();
  aliased();
  const arrow = (): V => sideEffect();
  arrow();

  // A `void`-returning callback in a `void`-declared slot.
  const items: number[] = [1, 2];
  let seen = 0;
  items.forEach((x: number): void => {
    seen = seen + x;
  });
  assert(seen === 3, "a void callback runs for its effects");

  let emitted = "";
  drain({ emit: (x: string): void => { emitted = x; } });
  assert(emitted === "a", "Sink<void> instantiates and dispatches");

  assert(true, "every void-return spelling compiles and runs");
}
