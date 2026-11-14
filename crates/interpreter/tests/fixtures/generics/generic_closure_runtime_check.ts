// A generic closure's own type parameter is checked at run time: forwarding
// `T` into `session.get<T>` uses the descriptor the call binds, through a
// direct call, an inner closure and a caller's own type parameter.
import session from "submilli:session";

interface Progress {
  step: number;
}

const store = {
  load<T>(key: string): T | undefined {
    return session.get<T>(key);
  },
  later<T>(key: string): () => T | undefined {
    return () => session.get<T>(key);
  },
};

function forward<U>(key: string): U | undefined {
  return store.load<U>(key);
}

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

function main(): void {
  session.set("k", 1);
  assert(store.load<number>("k") === 1, "a number reads back as a number");
  assert(rejects(() => { store.load<Progress>("k"); }), "the same key fails another type");
  assert(store.later<number>("k")() === 1, "an inner closure keeps the descriptor");
  assert(rejects(() => { store.later<string>("k")(); }), "an inner closure checks it");
  assert(forward<number>("k") === 1, "a caller forwards its own parameter");
  assert(rejects(() => { forward<string>("k"); }), "a forwarded parameter is checked");
}
