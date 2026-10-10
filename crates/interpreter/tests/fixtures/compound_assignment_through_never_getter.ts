// A compound assignment through an accessor checks the result against the
// setter's type, as tsc does, even when the getter is typed `never`.
class Locked {
  written: number = 0;
  get value(): never {
    throw new Error("locked");
  }
  set value(next: number) {
    this.written = next;
  }
}

function main(): void {
  const locked = new Locked();
  let threw = false;
  try {
    locked.value += 1;
  } catch (e) {
    threw = true;
  }
  assert(threw && locked.written === 0, "the getter throws before the setter runs");
}
