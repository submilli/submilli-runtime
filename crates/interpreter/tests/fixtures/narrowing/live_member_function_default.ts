function join(separator: string = "function"): string { return separator; }
class Joiner { join: (separator: string) => string = join; }
let value: number[] | Joiner = [1];
function change(): boolean { value = new Joiner(); return false; }
function main(): void {
  if (!Array.isArray(value) || change()) return;
  assert(value.join() === "function");
}
