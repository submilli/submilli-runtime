function main(): number {
  let failures = 0;
  {
    const read = (): number => late;
    try { read(); failures++; } catch (e: unknown) {
      if (!(e instanceof ReferenceError)) failures++;
    }
    const late = 7;
    if (read() !== 7) failures++;
  }
  const late = 1;
  {
    const read = (): number => late;
    const late = 2;
    if (read() !== 2) failures++;
  }
  let first: () => number = () => -1;
  let second: () => number = () => -1;
  for (let i = 0; i < 2; i++) {
    const read = (): number => late;
    const late = i;
    if (i === 0) first = read;
    else second = read;
  }
  if (first() !== 0 || second() !== 1) failures++;
  {
    const pick = (read: boolean): number => read ? late : 0;
    const early = pick(false);
    const late = 5;
    if (early + pick(true) !== 5) failures++;
  }
  for (let read = (): number => late, late = 7; late < 8; ) {
    late = 8;
    if (read() !== 7) failures++;
    break;
  }
  return failures;
}
