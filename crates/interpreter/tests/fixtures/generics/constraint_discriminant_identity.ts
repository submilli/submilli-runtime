class Left { kind: "left" = "left"; value: number = 1; }
class Right { kind: "right" = "right"; value: number = 2; }
function keep<T extends Left | Right>(value: T): T {
  if (value.kind === "left") { return value; }
  return value;
}
function guarded<T extends Left | Right | undefined>(value: T): T {
  if (value === undefined) { return value; }
  switch (value.kind) {
    case "left": return value;
    default: return value;
  }
}
function optional<T extends Left | Right | undefined>(value: T): T {
  if (value?.kind === "left") { return value; }
  return value;
}
function union<T extends Left, U extends Right>(value: T | U): T | U {
  if (value.kind === "left") { return value; }
  return value;
}
function optionalUnion<T extends Left, U extends Right>(value: T | U | undefined): T | U | undefined {
  if (value?.kind === "left") { return value; }
  return value;
}
function main(): void {
  const left = new Left();
  const right = new Right();
  const keptLeft: Left = keep(left);
  const keptRight: Right = keep(right);
  assert(keptLeft.value === 1 && keptRight.value === 2, "member lookup preserves constrained identity");
  assert(guarded(left).value === 1 && guarded(right).value === 2, "refinement preserves constrained identity");
  assert(guarded(undefined) === undefined, "nullish branch preserves identity");
  assert(optional(left).value === 1 && optional(right).value === 2 && optional(undefined) === undefined, "optional receiver preserves identity");
  assert(union<Left, Right>(left).value === 1 && union<Left, Right>(right).value === 2, "union receiver preserves identity");
  assert(optionalUnion<Left, Right>(left)?.value === 1 && optionalUnion<Left, Right>(undefined) === undefined, "optional union preserves identity");
}
