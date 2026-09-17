// Host application client (Node). Not a Submilli package.
export declare function getOrder(orderId: string): Promise<{ id: string; customerId: string; status: string; totalCents: number }>;
export declare function listCustomerOrders(customerId: string, cursor?: string): Promise<unknown>;
export declare function refundOrder(orderId: string, amountCents: number): Promise<string>;
export declare const ORDERS_API_TOKEN: string; // read from process.env at startup
