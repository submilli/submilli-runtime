import { Box } from "@test/lib";

function main(): void {
  const b = new Box<number>(4);
  assert(b.value === 4, "imported generic class constructs");
  assert(b.applied((x: number): number => x + 1) === 5, "closure-typed param over T");
  assert(b.mapper()(7) === 7, "closure-typed return over T");
}
