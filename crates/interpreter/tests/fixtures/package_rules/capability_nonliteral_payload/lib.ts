// expect-error: payload key `total` missing from `@capability` binding
import { check } from "submilli:security";
/**
 * Validates a purchase.
 * @param customer Customer identifier.
 * @param total Purchase total.
 * @capability acme.com/v { customer }
 */
export function v(customer: string, total: number): void {
  const context = { customer, total };
  check("acme.com/v", context);
}
