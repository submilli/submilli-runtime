function main(): number {
  const even = (n: number): boolean => n === 0 ? true : odd(n - 1);
  const odd = (n: number): boolean => n === 0 ? false : even(n - 1);
  let fib = (n: number): number => n < 2 ? n : fib(n - 1) + fib(n - 2);
  if (!even(4) || odd(4) || fib(6) !== 8) return 1;
  return 0;
}
