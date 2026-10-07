# lsuite AI: the agents of lsuite Pass

Decided by the owner on 2026-10-06: the apps stay free and open source, and lsuite sells one thing,
**AI inside the apps without setup**. Since 2026-10-07 that one thing is **lsuite Pass**
([PASS.md](PASS.md)): lsuite AI, lsuite Cloud ([CLOUD.md](CLOUD.md)) and the lsuite Marketplace
([MARKETPLACE.md](MARKETPLACE.md)) in one plan. This file is lsuite AI's contract (the provider in
the apps keeps that name) and the accounts' and billing's, which the whole Pass shares. A person installs an app, signs in once, and the agents work:
no Claude Code to install, no API key to paste. Bringing your own (Claude Code, Codex, API keys,
Ollama) stays free and is never pushed aside.

**For now this is a demo.** No payment is taken: checkout says so and activates the plan at once.
The owner approved these monthly prices on 2026-10-07: **$12 / $29 / $79 USD**.

## Plans

The plans of lsuite Pass (PASS.md):

| Plan | Monthly price (USD) | Models | Monthly allowance | lsuite Cloud | lsuite Marketplace |
| --- | --- | --- | --- | --- | --- |
| **Free** | 0 | bring your own provider | — | — | browse, publish |
| **Plus** | $12 / month | Claude Sonnet, Claude Haiku | 1,000 credits | 50 GB | install |
| **Pro** | $29 / month | + Claude Opus | 4,000 credits | 250 GB | install |
| **Studio** | $79 / month | everything, priority | 12,000 credits | 1 TB | install |

A credit is a fixed amount of model usage (input and output tokens weighted by the model's price;
for now half a US cent at the provider's list price, cache writes at 1.25× input, cache reads at
their own price); `GET /api/ai/plans` is the source of truth the apps and the site read, never a
copy in an app. The allowance resets on the first day of each month (UTC).

## lsuite Cloud and the lsuite Marketplace

A paid plan also includes installing from the lsuite Marketplace (MARKETPLACE.md, `/api/marketplace`;
`plans[].marketplace`), and cloud storage (the sizes above, decided on 2026-10-07; capped small in
the demo: 100 MB per account), managed from the lsuite launcher: see [CLOUD.md](CLOUD.md) for the
`/api/cloud` API, its errors and how files are stored. `GET /api/ai/plans` gives each plan's
`storage`, `GET /api/account/me` adds `cloud: {used, quota, files}`.

## One account for the whole suite

- Signing in from any app signs in every lsuite app on the machine. The account lives in
  `~/.lsuite/account.json` (0600; `LSUITE_HOME` replaces `~/.lsuite`):
  `{format: 1, server, email, name, plan, token, signedInAt}`. The token is a secret: never logged,
  never in a document, masked in diagnostics. Apps read the file when they need it (it can change
  under them) and write it atomically.
- `server` defaults to `https://lsuite.xyz`; `LSUITE_ACCOUNT_SERVER` overrides it (tests, a local
  demo server such as `http://127.0.0.1:4321`).

## Signing in (loopback, like native apps do OAuth)

1. The app listens on `127.0.0.1:<random port>` and opens
   `<server>/account/connect?app=<app>&port=<port>&state=<random>` in the browser (`<app>`: one of
   the five, or `lsuite` for the launcher).
2. The person signs in or creates the account there (demo: email and name), picks a plan if
   they have none (demo checkout), and presses **Connect <app>**.
3. The page sends the browser to `http://127.0.0.1:<port>/callback?code=<code>&state=<state>`;
   the app checks `state`, then `POST <server>/api/account/token {code}` → `{token, account}`.
4. Fallback for CLIs and headless use: the account page shows a key (`lsk_…`) to paste
   (`<app>-cli account.signIn key=…`).

## API (served by the site, `server.js`)

| Route | Does |
| --- | --- |
| `GET /api/ai/plans` | Plans, prices, models, allowances, storage, `marketplace`, `product: {name: "lsuite Pass", …}`, `demo: true`. |
| `GET /api/account/me` | With `Authorization: Bearer <token>`: `{email, name, plan, status, usage: {used, limit, resetsAt}, models, cloud: {used, quota, files}}`. |
| `POST /api/account/token` | `{code}` → `{token, account}` (codes live 5 minutes, used once). |
| `POST /api/account/signout` | Revokes the token. |
| `POST /api/ai/v1/messages` | **The Anthropic Messages API**, streaming included, for the plan's models; auth by `x-api-key: <token>` or `Authorization: Bearer <token>`. In validated production mode, checks the paid subscription and remaining allowance, forwards to Anthropic with `LSUITE_ANTHROPIC_API_KEY`, and counts usage. Demo mode always returns a clearly marked simulated response; adding a provider key alone does not activate forwarding. |
| `GET /api/ai/v1/models` | The plan's models. |

Model ids are the Anthropic ids (`claude-sonnet-5-5`, `claude-opus-5-5`, `claude-haiku-4-5`…;
dated ids resolve to the id they start with); `plans[].defaultModel` is the one to preselect.

### Errors

Every error is in Anthropic's shape, `{type: "error", error: {type, message, …}}`, and the
`message` is one line an app can show as it is. lsuite's own types:

| Status | `error.type` | When | Extra fields |
| --- | --- | --- | --- |
| 401 | `authentication_error` | Missing, unknown or revoked token: sign in again. | |
| 402 | `allowance_exhausted` | This month's credits are used up. Show **Manage plan**. | `manage_url`, `plan`, `resets_at`, `used`, `limit` |
| 403 | `plan_required` | The account is on Free (bring your own): "lsuite AI comes with lsuite Pass: …". | `manage_url`, `plan` |
| 403 | `model_not_in_plan` | The model is in a bigger plan. | `manage_url`, `plan`, `model` |
| 404 | `not_found_error` | No such model in lsuite AI. | |
| 400 | `invalid_grant` | `POST /api/account/token`: the code is unknown, used or expired. | |
| 429 | `rate_limit_error` | Too many sign-ins or code exchanges from one address (`Retry-After`). | |

Errors from Anthropic itself (overloaded, invalid request…) pass through unchanged.

### The site's own routes

The account pages (`/account`, `/account/connect`, `/account/checkout`, `/pass`; `/ai` is a 301 to `/pass`) use a session
cookie (`lsuite_session`, HttpOnly, SameSite=Lax) and JSON calls from the same origin:
`POST /api/account/session {email, name}` (demo sign-in or creation, no password),
`POST /api/account/checkout {plan}` (demo: no payment, the plan starts at once with a fresh
allowance), `POST /api/account/connect {app}` → `{code}` (for the loopback redirect),
`POST /api/account/key` → `{key}` (the paste key; a new one replaces the last),
`POST /api/account/disconnect {id}`, and `GET /api/account/me` with the cookie, which adds the
account's `connections` and `admin` (its email is in `LSUITE_ADMIN_EMAILS`: it reviews the
marketplace). Storage is one JSON file in `LSUITE_DATA_DIR` (0600, written atomically;
in memory when unset) holding only SHA-256 hashes of tokens and codes.

Because the endpoint speaks Anthropic's API, an app reaches it with the Anthropic provider it
already has (base URL `<server>/api/ai`, the token as key), and Claude Code runs through it with
`ANTHROPIC_BASE_URL=<server>/api/ai` and `ANTHROPIC_AUTH_TOKEN=<token>`.

## In every app

- **lsuite AI** is the first provider in the agent's provider list and in first-run setup, with
  the lsuite mark: "No setup. Sign in and your agent works." Signed in, it shows the plan and the
  allowance used (`Pro · 38 % used · resets 1 Nov`), **Manage plan** (opens `<server>/account`)
  and **Sign out**.
- Commands: `account.status`, `account.signIn` (opens the browser, or takes `key`),
  `account.signOut`, `account.plans`.
- When the allowance runs out, the agent's error says so in one line with **Manage plan**; it
  never silently switches to another provider.

## Production activation

The prices are final: Plus **$12 USD**, Pro **$29 USD**, Studio **$79 USD**, billed monthly.
The owner has not created Stripe, Anthropic API or transactional email accounts yet. The public
site therefore remains in demo mode and takes no payments. Setting an Anthropic key alone never
enables paid model calls. Production uses a separate `production-accounts.json`; demo accounts,
plans, sessions and app keys are not promoted into paid accounts.

Set these variables privately on the existing Railway `lsuite-site` service after creating the
provider accounts (do not put secret values in Git, issues or chat):

| Variable | Value |
| --- | --- |
| `LSUITE_MODE` | `production` (set last) |
| `LSUITE_PUBLIC_ORIGIN` | `https://lsuite.xyz` |
| `LSUITE_DATA_DIR` | `/data/lsuite` on the persistent volume, one service replica |
| `LSUITE_ANTHROPIC_API_KEY` | Funded Anthropic API account key |
| `LSUITE_STRIPE_SECRET_KEY` | Stripe live secret key |
| `LSUITE_STRIPE_WEBHOOK_SECRET` | Signing secret for `/api/billing/webhook` |
| `LSUITE_STRIPE_PRICE_PLUS` | Monthly USD Price, amount `1200` cents |
| `LSUITE_STRIPE_PRICE_PRO` | Monthly USD Price, amount `2900` cents |
| `LSUITE_STRIPE_PRICE_STUDIO` | Monthly USD Price, amount `7900` cents |
| `LSUITE_RESEND_API_KEY` | Resend transactional email API key |
| `LSUITE_EMAIL_FROM` | Sender on a domain verified in Resend, e.g. `lsuite <accounts@lsuite.xyz>` |

lsuite Cloud: with only the volume, production keeps the demo storage caps. For the plans' sizes
(up to 1 TB), create an object-store bucket and set the `LSUITE_CLOUD_S3_*` variables the same way
(CLOUD.md, "The object store").

Stripe: activate the account, create the three recurring prices, enable the customer portal for
payment methods, invoices, plan changes and cancellation, and add an event destination for
`customer.subscription.created`, `customer.subscription.updated`,
`customer.subscription.deleted`, `invoice.paid` and `invoice.payment_failed`.
Use event API version `2025-06-30.basil`, matching the integration. Stripe Checkout handles card
entry; the site never receives card details. Prices are validated before checkout. Billing
configuration and any applicable tax registrations must be completed in the Stripe account.

Resend: verify the sending domain using its DNS records, then configure the sender. Sign-in sends
an eight-digit code, valid once for ten minutes with at most five attempts. Codes and session/app
keys are hashed at rest. Session cookies are HttpOnly, SameSite=Lax and Secure in production.

Only signed Stripe events grant access, after retrieving the subscription's current state and
checking its customer, user, price, paid invoice and period. Duplicate/out-of-order events cannot
reset usage or restore cancelled access. In production the allowance resets at the subscription's
billing period, rather than the demo's calendar month. Subscription changes go through the portal;
usage within the same billing period is preserved. Studio requests have queue priority when the
gateway is busy. The gateway caps concurrent work and checks request size against remaining credit.

Run `npm test` before activation. The production integration tests simulate email delivery, signed
webhooks, duplicate checkout, USD price validation, cancellation and provider responses without
real credentials. A Stripe test-mode checkout and real email-delivery test are still required once
the accounts exist, before accepting live payments. This has not been tested against live accounts.

Provider references: [Stripe Checkout](https://docs.stripe.com/api/checkout/sessions/create),
[subscription events](https://docs.stripe.com/billing/subscriptions/webhooks),
[webhook signatures](https://docs.stripe.com/events/manage-webhook-endpoints),
[Resend email API](https://resend.com/docs/api-reference/emails/send-email).
