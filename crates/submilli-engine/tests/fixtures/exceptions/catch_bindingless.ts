class Special extends Error {}
function main(): void {
  let count = 0;
  const e = 7;
  try { throw new Error("x"); } catch { count += e; }
  try { throw new Special("x"); }
  catch (error: Special) { count += 1; }
  catch
  { count += 100; }
  finally { count += 2; }
  try { throw new Error("x"); } catch {
    try { throw new Error("y"); } catch { count += 4; }
  }
  try { count += 8; } catch { count += 100; }
  assert(count === 22);
}
