export interface Session { userId: string; customerId: string; role: "support" | "finance" }
// Supplied by the application's existing authentication middleware.
export declare function requireSession(request: Request): Promise<Session>;
