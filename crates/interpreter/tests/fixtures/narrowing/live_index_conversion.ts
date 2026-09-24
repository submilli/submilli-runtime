let log: string = "";
class Key {
  private reads: number = 0;
  valueOf(): number { log += "v"; return 0; }
  toString(): string {
    log += "s";
    this.reads += 1;
    return this.reads === 1 ? "0" : "1";
  }
}
let index: number | Key = 0;
function change(): boolean { index = new Key(); return false; }
function rhs(): number { log += "r"; return 3; }
function main(): void {
  const values = [4, 10];
  if (typeof index === "number" && !change()) values[index] += rhs();
  assert(log === "srs");
  assert(values[0] === 4);
  assert(values[1] === 7);
  index = 0;
  log = "";
  if (typeof index === "number" && !change()) values[index] = rhs();
  assert(log === "rs");
  assert(values[0] === 3);
  index = 0;
  log = "";
  let previous = 0;
  if (typeof index === "number" && !change()) previous = values[index]++;
  assert(previous === 3);
  assert(log === "ss");
  assert(values[0] === 3);
  assert(values[1] === 4);
}
