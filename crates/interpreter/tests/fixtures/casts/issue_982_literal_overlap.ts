function text(value: "" | boolean): string { return value as string; }
function numeric(value: 1 | boolean): number { return value as number; }
function main(): void {
  assert(text("") === "", "string literal overlap");
  assert(numeric(1) === 1, "number literal overlap");
  let caught = false;
  try { text(true); } catch (e: Error) { caught = true; }
  assert(caught, "checked casts still reject an incompatible member");
}
