interface Bag { tag: string; note?: string; }
class Acc implements Bag {
  tag: string = "a";
  private held: string = "i";
  get note(): string { return this.held; }
  set note(v: string) { this.held = v; }
  private get secret(): string { return "hidden"; }
}
class Child extends Acc { extra: number = 2; }
class Ordered {
  z: number = 0;
  get a(): number { this.z = this.z + 1; return this.z; }
  set only(value: number) { this.z = value; }
}
class Throws {
  get bad(): string { throw new Error("getter failed"); }
}
function main(): void {
  const bag: Bag = new Acc();
  assert(JSON.stringify(bag) === '{"note":"i","tag":"a"}', "public getter, private field");
  bag.note = "changed";
  assert(JSON.stringify(bag) === '{"note":"changed","tag":"a"}', "live getter");
  assert(JSON.stringify(new Child()) === '{"extra":2,"note":"i","tag":"a"}', "inherited properties");
  const ordered = new Ordered();
  assert(JSON.stringify(ordered) === '{"a":1,"z":1}', "canonical getter evaluation order");
  const widened = (new Acc() as unknown) as { tag: string; added?: string };
  widened.added = "value";
  assert(JSON.stringify(widened) === '{"added":"value","note":"i","tag":"a"}', "dynamic fields retain property policy");
  let caught = false;
  try { JSON.stringify(new Throws()); } catch (e) { caught = e.message === "getter failed"; }
  assert(caught, "getter errors propagate");
}
