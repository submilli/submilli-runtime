import { Printable, Serializable } from "@test/conversions";

class DerivedPrintable extends Printable { extra: number = 1; }
class DerivedSerializable extends Serializable { extra: number = 1; }
interface StringAlias { toString?(): string; }
interface JsonAlias { toJson?(): string; }
function replaceString(value: StringAlias): void { value.toString = (): string => "new"; }
function replaceJson(value: JsonAlias): void { value.toJson = (): string => "2"; }

function main(): void {
  const printable = new DerivedPrintable();
  replaceString(printable);
  assert(String(printable) === "new");
  const serializable = new DerivedSerializable();
  replaceJson(serializable);
  assert(JSON.stringify(serializable) === "2");
}
