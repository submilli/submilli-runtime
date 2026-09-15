export class FieldBase {
  v: string = "b";
}

export class MethodBase {
  m(): string {
    return "b";
  }
}

export class AccessorBase {
  private backing: string = "b";
  get a(): string {
    return this.backing;
  }
  set a(x: string) {
    this.backing = x;
  }
}

// An intermediate the consumer extends but does not itself redeclare in.
export class Middle extends FieldBase {
  extra: string = "mid";
}
