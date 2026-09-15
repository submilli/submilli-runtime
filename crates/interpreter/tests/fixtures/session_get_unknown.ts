// `get<unknown>` asks for a validated read against a type that admits every
// value, so the structural test would be vacuous. The cast machinery treats an
// `unknown` target as a no-op widen and emits no test at all, so accepting this
// would hand back an unvalidated value wearing a validated call's shape. Reject
// it at compile time and point at the untyped overload instead.
// expect-error: `session.get<unknown>` would not verify anything
import session from "submilli:session";

function main(): void {
  session.set("k", 1);
  const v = session.get<unknown>("k");
  assert(v === null || v !== null, "unreachable");
}
