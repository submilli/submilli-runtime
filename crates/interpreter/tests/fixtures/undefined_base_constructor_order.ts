let trace = "";
function mark(label: string): number { trace += label; return 7; }
class Base {
  field: number = mark("field;");
  constructor(public value: number = mark("default;"), public copied: number = this.field) {
    trace += "body;";
  }
}
class Derived extends Base {
  child: number = mark("child;");
  constructor(value: number = mark("derived-default;")) {
    super(value);
    trace += "derived-body;";
  }
}
function main(): void {
  const base = new Base();
  assert(trace === "field;default;body;", "base fields precede parameter defaults");
  assert(base.value === 7 && base.copied === 7, "parameter properties copy initialized defaults");
  trace = "";
  const supplied = new Base(9);
  assert(supplied.value === 9 && supplied.copied === 7);
  assert(trace === "field;body;", "supplied arguments still follow field initialization");
  trace = "";
  const derived = new Derived();
  assert(trace === "derived-default;field;body;child;derived-body;", "derived fields remain after super");
  assert(derived.value === 7 && derived.child === 7);
}
