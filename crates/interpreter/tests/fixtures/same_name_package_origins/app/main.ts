// expect-error-count: 1
// expect-error: expected `PaymentCard` from `@appworld/venmo`, got `PaymentCard` from `@appworld/supervisor`
import { show_payment_cards as show_sup_cards } from "@appworld/supervisor";
import { type PaymentCard } from "@appworld/venmo";
function main(): void {
  const supCards: PaymentCard[] = [];
  for (const c of show_sup_cards()) supCards.push(c);
}
