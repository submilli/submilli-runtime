export interface Session { userId: string; customerId: string; role: "support" | "ops" }
export declare function requireSession(request: Request): Promise<Session>;
