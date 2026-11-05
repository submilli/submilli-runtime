function mine(): number { interface Row { a: number; } const r: Row = { a: 1 }; return r.a; }
// Each module's local types are its own, even at the same offset as another
// module's.
import { other } from "./m_other";

/** Adds both modules' rows. */
export function run(): number {
  return mine() + other();
}
