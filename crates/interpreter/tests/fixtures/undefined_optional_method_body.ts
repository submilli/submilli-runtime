class Receiver {
  value: number = 3;
  read?(): number { return this.value; }
}
class Child extends Receiver {}
function main(): void {
  const receiver = new Receiver();
  assert(receiver.read?.() === 3, "implemented optional method can be called conditionally with its receiver");
  const child = new Child();
  child.value = 6;
  assert(child.read?.() === 6, "inherited optional method retains the child receiver");
}
