class Base { value: number | null = 1; }
class Child extends Base { value: number = 2; }
function main(): void {
  const child = new Child();
  const base: Base = child;
  const record: Record<string, number | null> = child;
  base.value = null;
  const key: string = "value";
  let caught = false;
  try { const value = record[key]; } catch (e: Error) { caught = true; }
  assert(caught);
  record["extra"] = 4;
  caught = false;
  try { const value = record[key]; } catch (e: Error) { caught = true; }
  assert(caught);
}
