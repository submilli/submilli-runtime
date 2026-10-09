let count = 0;
function next(): number { count++; return count; }
function choose(value: number | null = next()): number | null { return value; }
function optional(value?: string): string | undefined { return value; }
function required(value: string | undefined): string | undefined { return value; }
function earlier(a: number = next(), b: number = a + 1): number { return a + b; }
function main(): void {
  assert(choose() === 1);
  assert(choose(undefined) === 2);
  assert(choose(null) === null && count === 2);
  const indirect = choose;
  assert(indirect() === 3 && indirect(undefined) === 4);
  const closure = (value: number = next()): number => value;
  assert(closure() === 5);
  assert(earlier(10) === 21 && count === 5);
  assert(optional() === undefined && optional(undefined) === undefined);
  assert(required(undefined) === undefined);
  const source: { a?: number, b: number | null } = { b: null };
  const { a = 7, b = 8 } = source;
  assert(a === 7 && b === null, "destructuring only defaults undefined");
  const tuple: [number, number?] = [9];
  const [first, second = 10] = tuple;
  assert(first === 9 && second === 10);
}
