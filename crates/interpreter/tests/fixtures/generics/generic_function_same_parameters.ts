// A generic function value used as another generic function type with the
// same type parameters takes the call's type arguments by position, also
// when the target has more parameters.
import session from "submilli:session";

type Load = <U>(key: string, fallback?: number) => U | undefined;

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

function main(): void {
  const load: Load = <T,>(key: string): T | undefined => session.get<T>(key);
  session.set("k", 1);
  assert(load<number>("k") === 1, "a wider shape keeps the descriptors");
  assert(rejects(() => { load<string>("k"); }), "and checks them");
}
