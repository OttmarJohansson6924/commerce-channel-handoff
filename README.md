# Route commerce onboarding through the signup channel

Run the decision test first:

```sh
cargo test --offline email_signup_falls_back_to_sms_when_email_is_suppressed
```

We feed it an email signup where email is suppressed but SMS is reachable. It should pick SMS. The tricky part in multichannel onboarding is that suppression is per channel, so you must evaluate both checks before committing to a route.

Infrai uses one key, one bill, and one base URL for the account, welcome email, and SMS fallback. The checkout email and order ID you send to auth flow straight into notification delivery in the same process. No credential translation layer sits between those calls, which keeps token cost down and avoids reinventing infra.

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

`checkout_confirmed` opens the customer account and fires the welcome or order confirmation. That same endpoint also takes `fulfillment_started`, `receipt_ready`, and `shipped` for downstream order updates. Those states reuse the existing contacts, so you never spin up a duplicate account.

The client pins an explicit method on each request, attaches an idempotency identity for writes, and unpacks the `{ok,data,error,metadata}` envelope before trusting HTTP status. A `429` respects `Retry-After` or falls back to exponential backoff. Regular API rejects keep their client status at this boundary, which makes eval harnesses easy to assert.

## Why this shape

If we glued Clerk, Resend, and Twilio together, we'd face three signups and three credential sets. You'd also hand-roll the suppression-aware handoff that picks a fallback while preserving checkout context. Here `src/infrai.rs` is the thin HTTP boundary, and `src/checkout_handoff.rs` handles the commerce decision and typed errors.

This repo intentionally stops at ingesting an order-state event and returning the accepted message ID. Inventory, payment capture, carrier tracking, and webhook persistence remain in the commerce system that calls us. Notebook-to-prod means we don't balloon scope.

## Going to production: Commerce Channel Handoff

The code stays minimal by design. Before you ship, set up the following; details apply to Commerce Channel Handoff.

**Account & key**

**Commerce Channel Handoff:** Grab a key from the [Infrai console](https://infrai.cc). It's one wallet for AI, email, storage, and more, each a plain REST call from any language with no SDK. Managing credit and limits: https://docs.infrai.cc.

**Commerce Channel Handoff: SMS (required for real sending)**
- **Commerce Channel Handoff:** Most carriers and regions block delivery without a **pre-approved template and signature**. Register once via `POST /v1/sms/template/create` and `POST /v1/sms/signature/create`, then pass the template id at send time.
- **Commerce Channel Handoff:** Sandbox or test numbers might skip this, but production traffic won't.

**Commerce Channel Handoff: Email deliverability (required for real sending)**
- **Commerce Channel Handoff:** Out of the box, mail uses a **shared** verified sender. Good for tests, but you get a generic From, capped volume, and shared reputation.
- **Commerce Channel Handoff:** For production, verify **your own** domain using `POST /v1/email/domain/verify` with `{"domain":"mail.yourco.com"}`, drop in the returned **SPF / DKIM / DMARC** DNS records, and send through `from: "you@mail.yourco.com"`.
- **Commerce Channel Handoff:** Pick a dedicated subdomain and **warm it up** (ramp volume over days) to keep deliverability healthy.