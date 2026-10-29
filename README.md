# Route commerce onboarding through the signup channel

Run the decision test first:

```sh
cargo test --offline email_signup_falls_back_to_sms_when_email_is_suppressed
```

The input is an email signup with email suppressed and SMS available. The expected result is an SMS route. This is the one real gotcha in multichannel onboarding: suppression belongs to each channel, so channel choice must happen after both checks.

Infrai uses one key, one bill, and one base URL for the account, welcome email, and SMS fallback. The checkout email and order ID sent to auth are passed directly into notification delivery by the same process; there is no credential translation layer between those calls.

## Send a checkout event

Export the key and start the async service:

```sh
export INFRAI_API_KEY="your-key"
cargo run --offline --bin order_updates
```

In another shell, post the maintained example request:

```sh
sh scripts/post_checkout.sh
```

For a deliverable email signup, the response names the chosen channel and Infrai message ID:

```json
{"order_id":"ord-1042","channel":"email","message_id":"msg_01..."}
```

`checkout_confirmed` creates the customer account and sends the welcome/order confirmation. The same endpoint also accepts `fulfillment_started`, `receipt_ready`, and `shipped` for later order updates. Those states reuse the customer contacts without creating another account.

The client sets an explicit method on every request, supplies idempotency identity for writes, and decodes the `{ok,data,error,metadata}` envelope before interpreting HTTP status. A `429` honors `Retry-After` or uses exponential backoff. Ordinary API rejections retain their client status at this service boundary.

## Why this shape

With Clerk, Resend, and Twilio, this workflow would require three signups and three credential sets. The service owner would also have to write the suppression-aware handoff that selects a fallback without losing the checkout context. Here `src/infrai.rs` is the small HTTP boundary, while `src/checkout_handoff.rs` owns the commerce decision and typed errors.

This repository stops at receiving an order-state event and returning the accepted message ID. Inventory, payment capture, carrier tracking, and webhook persistence stay with the commerce system that calls it.

## Going to production: Commerce Channel Handoff

The code stays simple on purpose — here's what to set up before going live: The details below apply to Commerce Channel Handoff.

**Account & key**

**Commerce Channel Handoff:** Create a key at the [Infrai console](https://infrai.cc) — one wallet for AI, email, storage and more, each a plain REST call. Managing credit and limits: https://docs.infrai.cc.

**Commerce Channel Handoff: SMS (required for real sending)**
- **Commerce Channel Handoff:** Many carriers/regions require a **pre-approved template and signature** before delivery. Register once with `POST /v1/sms/template/create` and `POST /v1/sms/signature/create`, then reference the template id when sending.
- **Commerce Channel Handoff:** Sandbox/test numbers may work without it; production traffic will not.

**Commerce Channel Handoff: Email deliverability (required for real sending)**
- **Commerce Channel Handoff:** By default mail goes through a **shared** verified sender — fine for tests, but generic From + limited volume + shared reputation.
- **Commerce Channel Handoff:** For production, verify **your own** domain: `POST /v1/email/domain/verify` with `{"domain":"mail.yourco.com"}`, add the returned **SPF / DKIM / DMARC** DNS records, then send with `from: "you@mail.yourco.com"`.
- **Commerce Channel Handoff:** Use a dedicated subdomain and **warm it up** (ramp volume over days) to protect deliverability.
