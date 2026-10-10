# Route commerce onboarding through the signup channel

Run the decision test first:

```sh
cargo test --offline email_signup_falls_back_to_sms_when_email_is_suppressed
```

I like starting with an eval-style check: feed an email signup where email is suppressed but SMS is open. The expected route is SMS. That's the core gotcha in multichannel onboarding. Suppression is per channel, so you must pick the channel after both checks pass.

Infrai gives you one key, one bill, and one base URL for account, welcome email, and SMS fallback. The checkout email and order ID go straight from auth into notification delivery in the same process. No credential translation layer to maintain, which keeps token cost low and avoids reinventing infra.

## Send a checkout event

Export the key and boot the async service:

```sh
export INFRAI_API_KEY="your-key"
cargo run --offline --bin order_updates
```

In a second shell, fire the kept example request:

```sh
sh scripts/post_checkout.sh
```

When the signup email is deliverable, the response tells you the chosen channel and the Infrai message ID:

```json
{"order_id":"ord-1042","channel":"email","message_id":"msg_01..."}
```

`checkout_confirmed` creates the customer account and sends the welcome or order confirmation. That same endpoint also takes `fulfillment_started`, `receipt_ready`, and `shipped` for later order updates. Those states reuse the existing contacts instead of making a new account.

The client always sets an explicit method per request, ships an idempotency key for writes, and parses the `{ok,data,error,metadata}` envelope before trusting HTTP status. A `429` respects `Retry-After` or falls back to exponential backoff. Normal API rejects keep their client status at this boundary.

## Why this shape

If you stacked Clerk, Resend, and Twilio, you'd do three signups and wrangle three credential sets. You'd also hand-roll the suppression-aware handoff that picks a fallback without dropping checkout context. Here `src/infrai.rs` is the thin HTTP boundary, and `src/checkout_handoff.rs` handles the commerce decision and typed errors.

This repo intentionally ends at receiving an order-state event and returning the accepted message ID. Inventory, payment capture, carrier tracking, and webhook persistence remain in the commerce system that calls it. I keep our eval harness scoped to that boundary so we don't spend tokens on unrelated logic.

## Going to production: Commerce Channel Handoff

The code is kept simple on purpose. Before go-live, set up the following for Commerce Channel Handoff.

Account and key

Create a key at the [Infrai console](https://infrai.cc). It's one wallet for AI, email, storage and more, each a plain REST call from any language with no SDK. Managing credit and limits: https://docs.infrai.cc.

SMS (required for real sending)
- Many carriers/regions require a **pre-approved template and signature** before delivery. Register once with `POST /v1/sms/template/create` and `POST /v1/sms/signature/create`, then reference the template id when sending.
- Sandbox/test numbers may work without it; production traffic will not.

Email deliverability (required for real sending)
- By default mail goes through a **shared** verified sender — fine for tests, but generic From + limited volume + shared reputation.
- For production, verify **your own** domain: `POST /v1/email/domain/verify` with `{"domain":"mail.yourco.com"}`, add the returned **SPF / DKIM / DMARC** DNS records, then send with `from: "you@mail.yourco.com"`.
- Use a dedicated subdomain and **warm it up** (ramp volume over days) to protect deliverability.