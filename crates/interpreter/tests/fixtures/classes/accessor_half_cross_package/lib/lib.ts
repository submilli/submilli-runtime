export class Base {
  held: number = 10;
  heldY: number = 100;

  get x(): number | undefined {
    return this.held;
  }

  set x(v: number) {
    this.held = v;
  }

  get y(): number | undefined {
    return this.heldY;
  }

  set y(v: number) {
    this.heldY = v;
  }

  readX(): number | undefined {
    return this.x;
  }

  writeX(v: number): void {
    this.x = v;
  }
}

export class ExportedSetterOnly extends Base {
  last: number = 0;
  lastY: number = 0;

  set x(v: number) {
    this.last = v;
  }

  set y(v: number) {
    this.lastY = v;
  }
}

export class ExportedGetterOnly extends Base {
  get x(): number {
    return 99;
  }

  get y(): number {
    return 199;
  }
}
