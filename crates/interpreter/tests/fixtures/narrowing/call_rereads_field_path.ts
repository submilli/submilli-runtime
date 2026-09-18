class Mutable {
  value: string | null = "before";

  clear(): void {
    this.value = null;
  }

  clearDuringCondition(): boolean {
    this.value = null;
    return true;
  }

  readAfterCall(): string {
    if (this.value !== null) {
      this.clear();
      return this.value.toUpperCase();
    }
    return "unreachable";
  }

  readAfterConditionCall(): string {
    if (this.value !== null && this.clearDuringCondition()) {
      return this.value.toUpperCase();
    }
    return "unreachable";
  }
}

class AccessorMutation {
  value: string | null = "before";

  get clear(): number {
    this.value = null;
    return 1;
  }

  readAfterGetter(): string {
    if (this.value !== null) {
      const ignored = this.clear;
      return this.value + ignored.toString();
    }
    return "unreachable";
  }
}

class Leaf {
  value: string | null = "before";
}

class NestedMutation {
  inner: Leaf | null = new Leaf();

  clear(): void {
    this.inner = null;
  }

  readAfterCall(): string {
    if (this.inner !== null && this.inner.value !== null) {
      this.clear();
      return this.inner.value.toUpperCase();
    }
    return "unreachable";
  }
}

interface PlainValue {
  n: number;
}

interface MethodValue {
  n: number;
  go(): number;
}

class MethodValueImpl {
  n: number = 1;
  go(): number { return 2; }
}

function isMethodValue(value: PlainValue): value is MethodValue {
  return value instanceof MethodValueImpl;
}

class InterfaceMutation {
  value: PlainValue = new MethodValueImpl();

  clear(): void {
    this.value = { n: 5 };
  }

  readAfterCall(): number {
    if (isMethodValue(this.value)) {
      this.clear();
      return this.value.go();
    }
    return 0;
  }
}

export function main(): void {
  const value = new Mutable();
  try {
    value.readAfterCall();
    assert(false, "the stale narrowed value must not be returned");
  } catch (e) {
    assert(e instanceof TypeError, "a changed narrowed field throws TypeError");
  }

  const conditionValue = new Mutable();
  try {
    conditionValue.readAfterConditionCall();
    assert(false, "condition-side mutation must be checked at the first body use");
  } catch (e) {
    assert(e instanceof TypeError, "region entry must not raw-trap after condition mutation");
  }

  const accessor = new AccessorMutation();
  try {
    accessor.readAfterGetter();
    assert(false, "an accessor mutation must not leave a stale narrowed value");
  } catch (e) {
    assert(e instanceof TypeError, "an accessor mutation throws TypeError at the read");
  }

  const nested = new NestedMutation();
  try {
    nested.readAfterCall();
    assert(false, "a changed proper prefix must not raw-trap");
  } catch (e) {
    assert(e instanceof TypeError, "proper prefixes are checked before nested field reads");
  }

  const interfaceValue = new InterfaceMutation();
  try {
    interfaceValue.readAfterCall();
    assert(false, "an interface alternative changed by a call must be rejected");
  } catch (e) {
    assert(e instanceof TypeError, "interface field-path re-reads are structurally checked");
  }
}
