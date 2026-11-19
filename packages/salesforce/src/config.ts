// Build settings, chosen when the package is installed.

/**
 * The org's My Domain URL, such as `https://acme.my.salesforce.com`
 * (sandboxes: `https://acme--uat.sandbox.my.salesforce.com`), without a
 * trailing slash. Every request goes to this host, and the package's grants
 * name it.
 */
export declare const orgUrl: string;

/** The Salesforce REST API version every request uses: 68.0 is Winter '27. */
export const apiVersion = "68.0";
