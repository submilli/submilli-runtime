// A generic forwards its own `T` into `session.get<T>`: the caller names the
// type, so the descriptor the caller builds is what the stored value is
// checked against. Two instantiations check against their own types.
import session from "submilli:session";

interface Progress {
  step: number;
}

function load<T>(key: string): T | undefined {
  return session.get<T>(key);
}

// A type built from the parameter checks through the caller's descriptor too.
function loadAll<T>(key: string): T[] | undefined {
  return session.get<T[]>(key);
}

function main(): void {
  session.set("k", 1);
  const n = load<number>("k");
  assert(n === 1, "a number reads back as a number");

  let message = "";
  try {
    load<Progress>("k");
  } catch (e: TypeError) {
    message = e.message;
  }
  assert(message.indexOf("does not match `Progress`") >= 0, "the same key fails a different type");

  session.set("list", [1, 2]);
  const list = loadAll<number>("list");
  assert(list !== undefined && list.length === 2, "an array of the caller's type reads back");
  let listMessage = "";
  try {
    loadAll<string>("list");
  } catch (e: TypeError) {
    listMessage = e.message;
  }
  assert(listMessage.indexOf("does not match") >= 0, "an array of another type fails");
}
