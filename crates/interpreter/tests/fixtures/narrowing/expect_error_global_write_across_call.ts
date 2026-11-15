// expect-error: cannot read field `length`
let globalValue: string | number = "a";
function setGlobal(): void { globalValue = 5; }
function main(): string {
  if (typeof globalValue === "string") {
    setGlobal();
    return String(globalValue.length);
  }
  return "no";
}
