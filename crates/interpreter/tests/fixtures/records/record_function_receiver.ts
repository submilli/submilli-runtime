function main(): void {
  const name = "x";
  let calls = 0;
  const key = (): "x" => { calls += 1; return name; };
  const direct = { ["tag"]: 1, x: 3, read: function(): number { return this.x; } };
  const constant = { [name]: 4, read: function(): number { return this.x; } };
  const called = { [key()]: 5, read: function(): number { return this.x; } };
  assert(direct.read() === 3);
  assert(constant.read() === 4);
  assert(called.read() === 5);
  assert(calls === 1);
}
