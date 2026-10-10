// expect-error: `v` redeclares an inherited field as an accessor
// expect-error: `m` redeclares an inherited method as a field
// expect-error: `a` redeclares an inherited accessor as a method
// expect-error: `v` redeclares an inherited field as a method
import { AccessorBase, FieldBase, MethodBase, Middle } from "@test/kinds";

class AccOverImportedField extends FieldBase {
  get v(): string {
    return "c";
  }
}

class FieldOverImportedMethod extends MethodBase {
  m: string = "c";
}

class MethodOverImportedAccessor extends AccessorBase {
  a(): string {
    return "c";
  }
}

// Two hops: the grandparent declares it, the imported intermediate does not.
class MethodOverGrandparentField extends Middle {
  v(): string {
    return "c";
  }
}

function main(): void {}
