# Orders API (internal) — summary

Base URL: https://api.acme.com/v1. Auth: `Authorization: Bearer <token>`.
All responses are JSON. 404 for a missing resource; errors carry `{ "error": { "code", "message", "field" } }`.

| Method | Path | Notes |
| --- | --- | --- |
| GET | /customers/{customerId}/orders?limit=&cursor= | Newest first. Page: `{ items: Order[], nextCursor: string | null }`. limit 1-100, default 50 |
| GET | /orders/{orderId} | One order |
| POST | /orders/{orderId}/cancel | Body `{ reason?: string }`. Only `status: "open"` orders |
| POST | /orders/{orderId}/refund | Body `{ amountCents: number, reason?: string }`. Partial refunds allowed |
| GET | /orders?status=&since= | Cross-customer listing (operators only) |

Order: `{ id, customerId, status: "open" | "shipped" | "cancelled", totalCents, createdAt }`.
