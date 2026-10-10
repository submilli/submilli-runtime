function keeper(): void { let x: string | null = "a"; const mut = (): void => { x = null; }; mut(); }
function victim(x: string | null): number { if (x !== null) { return x.length; } return 0; }
function main(): void { keeper(); nested(); assert(parameterShadow("abc") === 3); assert(new Example().check("ab") === 2); assert(victim("hello") === 5, "unrelated closure does not poison binding"); }
function nested(): void {
  let x: string | null = "a";
  const mut = (): void => { x = null; };
  {
    const x: string | null = "inner";
    if (x !== null) { const get = (): string => x; assert(get() === "inner"); }
  }
  mut();
}
function parameterShadow(x: string | null): number {
  const mut = (x: string | null): void => { x = null; };
  mut(null);
  if (x !== null) { return x.length; }
  return 0;
}
class Example {
  check(x: string | null): number {
    if (x !== null) { return x.length; }
    return 0;
  }
}
