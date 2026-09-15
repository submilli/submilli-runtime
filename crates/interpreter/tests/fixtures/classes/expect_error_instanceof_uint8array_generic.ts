// expect-error: is always false: the types are not related
// A bare generic parameter is not related to `Uint8Array`, so the widened
// right-hand side still reports a diagnostic here rather than reaching the
// codegen arm — the `unreachable!` guarding non-class right-hand sides must
// stay unreachable. Same behavior a class right-hand side has.
function looksLikeBytes<T>(x: T): boolean {
  return x instanceof Uint8Array;
}

function main(): boolean {
  return looksLikeBytes<string>("bytes");
}
