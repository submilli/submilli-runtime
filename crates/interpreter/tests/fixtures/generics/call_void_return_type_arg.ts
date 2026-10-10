// A return-only type parameter admits an explicit void argument.
function run<T>(f: (x: number) => T, v: number): T {
  return f(v);
}

function main(): void {
  run<void>((x: number): void => {}, 1);
}
