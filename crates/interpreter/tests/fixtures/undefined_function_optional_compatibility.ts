function optionalView(value: (value: string | undefined) => number): (value?: string) => number {
  return value;
}
function main(): void {
  const original = (value: string | undefined): number => value === undefined ? 1 : value.length;
  const callback = optionalView(original);
  assert(callback() === 1, "optional view supplies undefined to a required union parameter");
  assert(callback("hello") === 5, "optional view preserves supplied argument");
  const required: (value: string | undefined) => number = callback;
  assert(required(undefined) === 1, "required union view accepts optional implementation");
}
