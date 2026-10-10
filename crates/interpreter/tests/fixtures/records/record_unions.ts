interface Base { [key: string]: number; }
interface Same extends Base { [key: string]: number; }
function read(values: Record<string, number> | Record<string, string>, key: string): number | string | undefined {
  return values[key];
}
function named(values: Record<string, number> | Record<string, string>): number | string | undefined {
  return values.item;
}
function main(): void {
  const numbers: Same = { item: 4 };
  const strings: Record<string, string> = { item: "four" };
  assert(read(numbers, "item") === 4);
  assert(read(strings, "item") === "four");
  assert(named(numbers) === 4);
  assert(named(strings) === "four");
}
