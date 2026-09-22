import { requireSession } from "./auth";
import { readBalance, refund, exportCustomers } from "./billing";

export async function agentContext(request: Request) {
  const session = await requireSession(request);
  return {
    session,
    // Current direct tools: adoption must account for these alternate paths.
    tools: { readBalance, refund, exportCustomers },
  };
}
