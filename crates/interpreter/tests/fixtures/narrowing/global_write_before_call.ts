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
function main(): void {
  assert(readBeforeCall() === 1, "guard remains valid before any call");
  globalValue = "abc";
  const snapshot = globalValue;
  if (typeof snapshot === "string") {
    setGlobal();
    assert(snapshot.length === 3, "const snapshot survives a call");
  }
}
