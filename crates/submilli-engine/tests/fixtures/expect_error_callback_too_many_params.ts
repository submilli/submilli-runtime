// A callback may declare fewer parameters than the call passes, not more.
// expect-error: expected `(arg0: string, arg1: number, arg2: string[]) => U`
function main(): void {
  const xs = ["a"];
  xs.map((a: string, i: number, all: string[], extra: number) => a);
}
