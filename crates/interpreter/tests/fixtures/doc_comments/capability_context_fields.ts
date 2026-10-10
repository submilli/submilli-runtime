import { check } from "submilli:security";

interface BaseContext { customer: string }
interface Context extends BaseContext { total?: number }
type ContextAlias = Context;

/**
 * Validates the statically declared context.
 * @param context Purchase context.
 * @returns The serialized context.
 * @capability acme.com/v { customer: string, total: number }
 */
function validate(context: ContextAlias): string {
  check("acme.com/v", context);
  return JSON.stringify(context);
}

function main(): void {
  assert(validate({ customer: "c", total: 50 }) === '{"customer":"c","total":50}');
  assert(validate({ customer: "c" }) === '{"customer":"c"}');
}
