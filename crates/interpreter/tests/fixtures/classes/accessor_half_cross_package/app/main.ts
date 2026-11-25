import { Base, ExportedGetterOnly, ExportedSetterOnly } from "@test/acc";

class LocalSetterOnly extends Base {
  seen: number = 0;

  set x(v: number) {
    this.seen = v;
  }
}

class LocalGetterOnly extends Base {
  get x(): number {
    return 77;
  }
}

class LocalGrandChildSetterOnly extends ExportedSetterOnly {}
class LocalGrandChildGetterOnly extends ExportedGetterOnly {}
class LocalGrandChildFlipToGetter extends ExportedSetterOnly {
  get x(): number {
    return 55;
  }
}

function readImportedOptional(b: Base | null): number | undefined {
  return b?.x;
}

function readGenericYOptional<T extends Base>(b: T | null): number | undefined {
  return b?.y;
}

function writeGenericY<T extends Base>(b: T, v: number): void {
  b.y = v;
}

function throwsTypeError(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof TypeError;
  }
}

function main(): void {
  const eso = new ExportedSetterOnly();
  const esoBase: Base = eso;
  esoBase.writeX(11);
  assert(eso.last === 11, "exported setter-only runs its own setter");
  assert(esoBase.x === undefined, "exported setter-only clears inherited getter");
  assert(esoBase.readX() === undefined, "base method sees cleared getter on exported subclass");

  const ego = new ExportedGetterOnly();
  const egoBase: Base = ego;
  assert(egoBase.x === 99, "exported getter-only runs its own getter");
  assert(
    throwsTypeError(() => {
      egoBase.x = 5;
    }),
    "exported getter-only clears inherited setter",
  );
  assert(
    throwsTypeError(() => {
      egoBase.writeX(5);
    }),
    "base method write throws TypeError on exported getter-only subclass",
  );

  const lso = new LocalSetterOnly();
  const lsoBase: Base = lso;
  lsoBase.writeX(22);
  assert(lso.seen === 22, "local setter-only over imported base runs its own setter");
  assert(lsoBase.x === undefined, "local setter-only over imported base clears getter");
  assert(lsoBase.readX() === undefined, "imported base read sees cleared getter on local subclass");

  const lgo = new LocalGetterOnly();
  const lgoBase: Base = lgo;
  assert(lgoBase.x === 77, "local getter-only over imported base runs its own getter");
  assert(
    throwsTypeError(() => {
      lgoBase.writeX(5);
    }),
    "imported base write throws TypeError on local getter-only subclass",
  );

  const gcSo: Base = new LocalGrandChildSetterOnly();
  assert(gcSo.x === undefined, "local grandchild of exported setter-only keeps cleared getter");
  assert(gcSo.readX() === undefined, "imported base read on local grandchild sees cleared getter");

  const gcGo: Base = new LocalGrandChildGetterOnly();
  assert(gcGo.x === 99, "local grandchild of exported getter-only keeps getter");
  assert(
    throwsTypeError(() => {
      gcGo.writeX(5);
    }),
    "local grandchild of exported getter-only keeps cleared setter",
  );

  // Optional-chain read on an imported class must still intern and call the getter.
  assert(
    readImportedOptional(new Base()) === 10,
    "optional-chain read on imported base invokes getter",
  );
  assert(readImportedOptional(null) === undefined, "optional-chain null short-circuits");
  assert(
    readImportedOptional(eso) === undefined,
    "optional-chain read on exported setter-only sees cleared getter",
  );

  // Local grandchild flipping an exported setter-only override back to getter-only.
  const flipped: Base = new LocalGrandChildFlipToGetter();
  assert(flipped.x === 55, "flipped grandchild getter answers");
  assert(
    throwsTypeError(() => {
      flipped.writeX(1);
    }),
    "flipped grandchild clears exported setter-only setter",
  );

  // Bounded generic reads/writes on an imported-only accessor (`y`, never
  // declared locally or accessed on a concrete class type in this module).
  const gb = new Base();
  writeGenericY(gb, 333);
  assert(
    readGenericYOptional(gb) === 333,
    "generic write and optional-chain read use imported-only accessors",
  );
  assert(
    readGenericYOptional(eso) === undefined,
    "generic optional-chain read sees cleared getter on exported setter-only",
  );
  writeGenericY(eso, 444);
  assert(eso.lastY === 444, "generic write invokes exported setter-only setter");
  assert(
    throwsTypeError(() => {
      writeGenericY(ego, 9);
    }),
    "generic write throws TypeError on exported getter-only",
  );
}
