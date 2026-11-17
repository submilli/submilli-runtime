type Label = string;
let calls = 0;
function custom(): string { return "c"; }
function value(): () => string { calls++; return custom; }
function writeAlias(s: Label): void { s.toString = value(); }
function writeString(s: string): void { s.toString = value(); }
function writeNumber(s: number): void { s.toString = value(); }
function writeBoolean(s: boolean): void { s.toString = value(); }
function genericString<T extends string>(s: T): void { s.toString = value(); }
function genericNumber<T extends number>(s: T): void { s.toString = value(); }
function genericBoolean<T extends boolean>(s: T): void { s.toString = value(); }
function narrowed<T>(s: T): void { if (typeof s === "boolean") s.toString = value(); }
function main(): number {
  let failures = 0;
  try { writeString("abc"); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { writeNumber(1); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { writeBoolean(true); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { genericString("abc"); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { genericNumber(1); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { genericBoolean(true); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { narrowed(false); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  try { writeAlias("abc"); } catch (e: unknown) { if (e instanceof TypeError) failures++; }
  return failures === 8 && calls === 8 ? 0 : 1;
}
