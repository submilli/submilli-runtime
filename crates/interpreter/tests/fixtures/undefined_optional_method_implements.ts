interface Reader { read?(): number; }

class Box implements Reader {
  value: number = 8;
  read(): number { return this.value; }
}

class Child extends Box implements Reader {}

function through(reader: Reader): number | undefined { return reader.read?.(); }

function main(): void {
  assert(through(new Box()) === 8, "ordinary class method implements an optional method");
  assert(through(new Child()) === 8, "inherited method implements an optional method");
  const mutable: Reader = new Box();
  mutable.read = (): number => 9;
  assert(through(mutable) === 9, "optional methods remain writable through the interface");
}
