function conditionAssignment(flag: boolean): number {
  const box: { x: string | null } = { x: null };
  if ((box.x = "ok").length === 2 && flag) { return box.x.length; }
  else { return box.x.length + 1; }
}

function assignInNullBranch(): string {
  const box: { x: string | null } = { x: "before" };
  box.x = null;
  if (box.x === null) { box.x = "after"; } else { return box.x; }
  return box.x;
}

function aliasReguard(): string {
 const box: { x: string | null } = { x: "before" };
 const alias = box;
 box.x = null;
 alias.x = "after";
 if (box.x !== null) { return box.x; }
 return "none";
}

function main(): void {
  assert(conditionAssignment(true) === 2 && conditionAssignment(false) === 3, "condition assignment with returning branches");
  assert(assignInNullBranch() === "after", "null equality branch remains writable");
  assert(aliasReguard() === "after", "alias mutation before a null re-guard preserves the live value");
  let x: { a: number; b?: string | null } = { a: 1 };
  x.b = "ok";
  assert(x.b.length === 2, "fresh optional field narrowing");
  x.b = null;
  assert(x.b === null, "null write");
  x.b = "again";
  assert(x.b.length === 5, "repeated write");
  const nested: { inner: { value: number | null } } = { inner: { value: null } };
  nested.inner.value = 3;
  assert(nested.inner.value + 1 === 4, "nested path");
  assert((x.b = "value").length === 5 && x.b.length === 5, "assignment expression");
  const box: { value: number | null } = { value: null };
  box.value = 7;
  assert(box.value + 1 === 8, "nullable number");
  if (x.a === 1) { box.value = 3; } else { box.value = 4; }
  assert(box.value > 0, "joined writes");
}
