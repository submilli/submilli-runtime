class InlineSource {
  tag: string = "a";
  private stored: number = 7;
  get value(): number { return this.stored; }
  set value(v: number) { this.stored = v; }
  read(): string { return this.tag; }
}
function readInline(x: { tag: string; value: number; read(): string; extra?: string }): void {
  assert(x.tag === "a", "field");
  assert(x.value === 7, "getter");
  x.value = 9;
  assert(x.read() === "a", "method");
}
class Inherited<T> { item: T; constructor(item: T) { this.item = item; } }
class Child extends Inherited<number> {}
function generic(x: {item: number; absent?: boolean}): number { return x.item; }
function main(): void {
  assert(generic(new Child(12)) === 12, "inherited generic field");
  const source = new InlineSource();
  readInline(source);
  assert(source.value === 9, "setter");
}
