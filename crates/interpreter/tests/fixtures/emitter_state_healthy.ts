function check(value: boolean, message: string): void {
  if (!value) { throw new Error(message); }
}

function counter(value: number): () => number {
  return (): number => { value += 1; return value; };
}

function main(): void {
  const next = counter(2);
  check(next() === 3 && next() === 4, "boxed parameter and capture");
  let total = 0;
  for (let i = 0; i < 4; i += 1) {
    try {
      switch (i) {
        case 0: continue;
        case 1: total += 10; break;
        default: total += i;
      }
    } finally {
      total += 1;
    }
  }
  check(total === 19, "loop, switch, continue and finally depths");
  let text: string | null = "ok";
  if (text !== null) { check(text.length === 2, "narrowed slot"); }
  text = null;
  check(text === null, "write clears narrowing");
  check("😀".length === 2, "UTF-16 literal");
}
