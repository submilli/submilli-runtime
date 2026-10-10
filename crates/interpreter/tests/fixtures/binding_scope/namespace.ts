import uuid from "submilli:uuid";
class Fake { label: string = "local"; v4(): string { return this.label; } }
function call(uuid: Fake): string { return uuid.v4(); }
function main(): void {
  const uuid = new Fake();
  assert(uuid.v4() === "local", "local method shadows namespace import");
  assert(uuid.label === "local", "local field shadows namespace import");
  assert(call(uuid) === "local", "parameter shadows namespace import");
}
