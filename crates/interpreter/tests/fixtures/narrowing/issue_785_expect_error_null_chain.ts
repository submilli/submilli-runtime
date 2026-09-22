// expect-error: the receiver can be `null`
interface Box { value: string | null; }
function read(b: Box | null): string | null {
  if (b?.value == null) { return b.value; }
  return "present";
}
export function main(): void {}
