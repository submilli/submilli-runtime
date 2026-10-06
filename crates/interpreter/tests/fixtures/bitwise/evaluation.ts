class Box {
  private stored: number = 7;
  log: string = "";
  get value(): number { this.log += "get;"; return this.stored; }
  set value(n: number) { this.log += "set;"; this.stored = n; }
}
function main(): void {
  const box = new Box();
  function receiver(): Box { box.log += "receiver;"; return box; }
  function rhs(): number { box.log += "rhs;"; return 3; }
  const assigned = (receiver().value &= rhs());
  assert(assigned === 3, "assignment result");
  assert(box.log === "receiver;get;rhs;set;", "accessor order");
  let order = "";
  const left = {valueOf: (): number => { order += "left;"; return 7; }};
  assert(~left === -8 && order === "left;", "coercion once");
}
