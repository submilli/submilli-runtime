interface Receiver<T> { take(value: T): void }
function take(receiver: Receiver<void>): void { receiver.take(undefined); }
function main(): void {
  let called = false;
  take({ take: (value: void): void => { called = value === undefined; } });
  assert(called, "interface method generic parameter accepts void");
}
