// A stored condition narrows only a constant reference, through an
// unannotated `const`, as in TypeScript: each test below reads the declared
// type.
// expect-error: cannot read field `length` on non-object type `number | string`
// expect-error: field `radius` does not exist on all members
// expect-error: field `radius` does not exist on all members
// expect-error: field `radius` does not exist on all members
// expect-error: field `radius` does not exist on all members
// expect-error-count: 5
type Shape = { kind: "circle"; radius: number } | { kind: "square"; side: number };

function assignedLater(box: { readonly value: string | number }): number {
  const isString = typeof box.value === "string";
  box = { value: 42 };
  return isString ? box.value.length : 0;
}

function annotatedAlias(shape: Shape): number {
  const isCircle: boolean = shape.kind === "circle";
  return isCircle ? shape.radius : 0;
}

function mutableAlias(shape: Shape): number {
  let isCircle = shape.kind === "circle";
  return isCircle ? shape.radius : 0;
}

function reassignedTarget(shape: Shape): number {
  const isCircle = shape.kind === "circle";
  shape = { kind: "square", side: 1 };
  return isCircle ? shape.radius : 0;
}

function mutableField(outer: { shape: Shape }): number {
  const isCircle = outer.shape.kind === "circle";
  return isCircle ? outer.shape.radius : 0;
}

function main(): void {}
