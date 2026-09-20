function parameter(value: number | null): number {
  if (value !== null) {
    const read = (): number => value;
    return read();
  }
  return 0;
}
function main(): void {
  let value: number | null = 5;
  if (value !== null) {
    const read = (): number => value;
    assert(read() === 5);
  }
  let assigned: number | null = null;
  assigned = 7;
  const readAssigned = (): number => assigned;
  assert(readAssigned() === 7);
  assert(parameter(9) === 9);
  assert(parameter(null) === 0);
  assert(nested(11) === 11);
  assert(shadowed() === 4);
  assert(perIteration() === 3);
}

function nested(value: number | null): number {
  if (value !== null) {
    const outer = (): number => {
      const inner = (): number => value;
      return inner();
    };
    return outer();
  }
  return 0;
}

function shadowed(): number {
  let value: number | null = 4;
  if (value !== null) {
    const read = (): number => value;
    {
      let value: number | null = 2;
      value = null;
      assert(value === null);
    }
    return read();
  }
  return 0;
}

function perIteration(): number {
  let total = 0;
  for (let i = 0; i < 3; i++) {
    let value: number | null = i;
    if (value !== null) {
      const read = (): number => value;
      total += read();
    }
  }
  return total;
}
