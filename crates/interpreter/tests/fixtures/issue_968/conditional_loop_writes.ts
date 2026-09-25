function count(explicitElse: boolean): number {
  let value: string | number | null = 'a';
  let outer = 0;
  let total = 0;
  while (value !== null && outer < 2) {
    let inner = 0;
    while (inner < 2) {
      if (typeof value === 'number') {
        total += value;
      } else {
        total += value.length;
      }
      if (explicitElse) {
        if (inner === 0) value = 2;
        else total += 1;
      } else {
        if (inner === 0) value = 2;
      }
      inner++;
    }
    value = 1;
    outer++;
  }
  return total;
}

function main(): void {
  assert(count(false) === 6, 'implicit else retains incoming non-null facts');
  assert(count(true) === 8, 'explicit unchanged branch retains incoming facts');
}
