function check(o: { a: number }, key: string): boolean {
  return key in o;
}

function main(): void {
  assert(check({ a: 1 }, "a") === true);
}
