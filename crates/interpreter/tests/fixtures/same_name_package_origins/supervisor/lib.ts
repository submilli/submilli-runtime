/** A supervisor payment card. */
export interface PaymentCard {
  /** The supervisor identifier. */
  other_id: string;
}
/** List supervisor cards.
 * @returns The supervisor cards.
 */
export function show_payment_cards(): PaymentCard[] { return [{ other_id: "x" }]; }
