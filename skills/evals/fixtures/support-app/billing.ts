// Host application client; these are not Submilli package implementations.
export declare function readBalance(customerId: string): Promise<number>;
export declare function refund(chargeId: string, amountCents: number): Promise<string>;
export declare function exportCustomers(): Promise<string>;
