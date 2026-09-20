function main(): void {
  const x = 1;
  const get = (): number => x;
  {
    let x: string | null = "inner";
    const write = (): void => { x = null; };
    write();
  }
  assert(get() === x);

  let value = 2;
  {
    const value = "inner";
    const read = (): string => value;
    assert(read() === "inner");
  }
  const increment = (): void => { value += 1; };
  increment();
  assert(value === 3, "outer mutable binding is restored and boxed");

  for (const [x] of [[4], [5]]) {
    const read = (): number => x;
    { let x = "inner"; const write = (): void => { x = "changed"; }; write(); }
    assert(read() === x, "destructured loop binding survives nested shadow");
  }
  try { throw new Error("test"); }
  catch (error: Error) {
    const read = (): string => error.message;
    { let error = 0; const write = (): void => { error += 1; }; write(); }
    assert(read() === error.message, "catch binding survives nested shadow");
  }
}
