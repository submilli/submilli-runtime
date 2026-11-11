// A generic class's method forwards the class's `T` into `session.get<T>`:
// each instance carries the type arguments it was created with, so two
// instantiations check the same stored value against their own types.
import session from "submilli:session";

class Slot<T> {
  key: string;
  constructor(key: string) {
    this.key = key;
  }
  load(): T | undefined {
    return session.get<T>(this.key);
  }
}

function main(): void {
  session.set("k", 1);
  const numbers = new Slot<number>("k");
  const strings = new Slot<string>("k");
  assert(numbers.load() === 1, "a Slot<number> reads a number back");

  let message = "";
  try {
    strings.load();
  } catch (e: TypeError) {
    message = e.message;
  }
  assert(message.indexOf("does not match `string`") >= 0, "a Slot<string> rejects the number");
}
