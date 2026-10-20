// expect-error: payload key `total` missing from `@capability` binding
import { check } from "submilli:security";
/**
 * Validates a purchase.
 * @param customer Customer identifier.
 * @param total Purchase total.
 * @capability pied-piper.example/v { customer }
 */
export function v(customer: string, total: number): void {
  const context = { customer, total };
  check("pied-piper.example/v", context);
}
