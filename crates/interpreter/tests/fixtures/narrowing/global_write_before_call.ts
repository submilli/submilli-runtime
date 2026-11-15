let globalValue: string | number = "a";
function setGlobal(): void { globalValue = 5; }
function readBeforeCall(): number {
  if (typeof globalValue === "string") {
    const length = globalValue.length;
    setGlobal();
    return length;
  }
  return 0;
}
function wholeValueAfterCall(): string {
  globalValue = "abc";
  setGlobal();
  return globalValue + "!";
}
function pureAssertion(): number {
  globalValue = "a";
  if (typeof globalValue === "string") {
    assert(globalValue.length === 1, "assert does not reassign module bindings");
    return globalValue.length;
  }
  return 0;
}
function deferredFunctionDoesNotRun(): number {
  globalValue = "a";
  if (typeof globalValue === "string" && [() => setGlobal()].length === 1) {
    return globalValue.length;
  }
  return 0;
}
function pureInvokedFunction(): number {
  globalValue = "abc";
  if (typeof globalValue === "string" && (() => true)()) {
    return globalValue.length;
  }
  return 0;
}
function main(): void {
  assert(deferredFunctionDoesNotRun() === 1, "creating an arrow does not run its body");
  assert(pureInvokedFunction() === 3, "a precise IIFE keeps the guard");
  assert(wholeValueAfterCall() === "5!", "whole reads see the value left by a call");
  assert(pureAssertion() === 1, "pure intrinsic preserves narrowing");
  globalValue = "a";
  assert(readBeforeCall() === 1, "guard remains valid before any call");
  globalValue = "abc";
  const snapshot = globalValue;
  if (typeof snapshot === "string") {
    setGlobal();
    assert(snapshot.length === 3, "const snapshot survives a call");
  }
}
