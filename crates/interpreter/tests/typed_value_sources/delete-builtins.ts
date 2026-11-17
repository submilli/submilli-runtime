function eraseStringMethod(o: { toString?: () => string }): boolean { return delete o.toString; }
function eraseLength(o: { length?: number }): boolean { return delete o.length; }
function eraseLastIndex(o: { lastIndex?: number }): boolean { return delete o.lastIndex; }
function main(): number {
  if (!eraseStringMethod(1) || !eraseStringMethod("abc") || !eraseStringMethod([1])) return 1;
  let caught = 0;
  try { eraseLength("abc"); } catch (e) { if (e instanceof TypeError) caught++; }
  try { eraseLength([1]); } catch (e) { if (e instanceof TypeError) caught++; }
  try { eraseLastIndex(/x/g); } catch (e) { if (e instanceof TypeError) caught++; }
  if (caught !== 3) return 2;
  return 0;
}
