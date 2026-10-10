// The words strict mode reserves stay valid as property names, enum members and
// modifiers.
enum Access { public, private }
class Box {
  constructor(public value: number, private readonly static_: number) {}
  static make(): Box { return new Box(1, 2); }
  yield(): number { return this.value + this.static_; }
}
type Flags = { public: boolean; package: string };
function main(): void {
  const flags: Flags = { public: true, package: "p" };
  const { public: isPublic } = flags;
  assert(isPublic && flags.package === "p", "reserved words as property names");
  assert(Box.make().yield() === 3, "reserved words as method names");
  assert(Access.private === 1, "reserved words as enum members");
}
