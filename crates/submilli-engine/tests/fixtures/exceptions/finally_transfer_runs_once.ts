let log = "";
function returnThrow(): string {
  try { return "body"; }
  catch (e) { log = log + "wrong;"; return "caught"; }
  finally { log = log + "finally;"; throw new Error("boom"); }
}
function loopThrow(continuing: boolean): void {
  let n = 0;
  while (n < 2) {
    n = n + 1;
    try { if (continuing) { continue; } break; }
    catch (e) { log = log + "wrong;"; }
    finally { log = log + "finally;"; throw new Error("boom"); }
  }
}
function main(): void {
  try { returnThrow(); } catch (e) { assert(e.message === "boom"); }
  assert(log === "finally;");
  log = "";
  try { loopThrow(false); } catch (e) { assert(e.message === "boom"); }
  assert(log === "finally;");
  log = "";
  try { loopThrow(true); } catch (e) { assert(e.message === "boom"); }
  assert(log === "finally;");
}
