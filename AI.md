# lsuite AI: the subscription

Decided by the owner on 2026-10-06: the apps stay free and open source, and lsuite sells one thing,
**AI inside the apps without setup**. A person installs an app, signs in once, and the agents work:
no Claude Code to install, no API key to paste. Bringing your own (Claude Code, Codex, API keys,
Ollama) stays free and is never pushed aside.

**For now this is a demo.** No payment is taken: checkout says so and activates the plan at once.
The prices below are placeholders the owner will set.

## Plans

| Plan | Price (demo) | Models | Monthly allowance |
| --- | --- | --- | --- |
| **Free** | 0 | bring your own provider | — |
| **Plus** | $12 / month | Claude Sonnet, Claude Haiku | 1,000 credits |
| **Pro** | $29 / month | + Claude Opus | 4,000 credits |
| **Studio** | $79 / month | everything, priority | 12,000 credits |

A credit is a fixed amount of model usage (input and output tokens weighted by the model's price;
for now half a US cent at the provider's list price, cache writes at 1.25× input, cache reads at
their own price); `GET /api/ai/plans` is the source of truth the apps and the site read, never a
copy in an app. The allowance resets on the first day of each month (UTC).

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
   `<server>/account/connect?app=<app>&port=<port>&state=<random>` in the browser.
2. The person signs in or creates the account there (demo: email and name), picks a plan if
   they have none (demo checkout), and presses **Connect <app>**.
3. The page sends the browser to `http://127.0.0.1:<port>/callback?code=<code>&state=<state>`;
   the app checks `state`, then `POST <server>/api/account/token {code}` → `{token, account}`.
4. Fallback for CLIs and headless use: the account page shows a key (`lsk_…`) to paste
   (`<app>-cli account.signIn key=…`).

## API (served by the site, `server.js`)

| Route | Does |
| --- | --- |
| `GET /api/ai/plans` | Plans, prices, models, allowances, `demo: true`. |
| `GET /api/account/me` | With `Authorization: Bearer <token>`: `{email, name, plan, status, usage: {used, limit, resetsAt}, models}`. |
| `POST /api/account/token` | `{code}` → `{token, account}` (codes live 5 minutes, used once). |
| `POST /api/account/signout` | Revokes the token. |
| `POST /api/ai/v1/messages` | **The Anthropic Messages API**, streaming included, for the plan's models; auth by `x-api-key: <token>` or `Authorization: Bearer <token>`. Checks the plan and the allowance, then forwards to Anthropic with the server's key (`LSUITE_ANTHROPIC_API_KEY`) and counts the usage. Without a server key the demo answers with a short streamed message saying so. |
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
| 403 | `plan_required` | The account is on Free (bring your own). | `manage_url`, `plan` |
| 403 | `model_not_in_plan` | The model is in a bigger plan. | `manage_url`, `plan`, `model` |
| 404 | `not_found_error` | No such model in lsuite AI. | |
| 400 | `invalid_grant` | `POST /api/account/token`: the code is unknown, used or expired. | |
| 429 | `rate_limit_error` | Too many sign-ins or code exchanges from one address (`Retry-After`). | |

Errors from Anthropic itself (overloaded, invalid request…) pass through unchanged.

### The site's own routes

The account pages (`/account`, `/account/connect`, `/account/checkout`, `/ai`) use a session
cookie (`lsuite_session`, HttpOnly, SameSite=Lax) and JSON calls from the same origin:
`POST /api/account/session {email, name}` (demo sign-in or creation, no password),
`POST /api/account/checkout {plan}` (demo: no payment, the plan starts at once with a fresh
allowance), `POST /api/account/connect {app}` → `{code}` (for the loopback redirect),
`POST /api/account/key` → `{key}` (the paste key; a new one replaces the last),
`POST /api/account/disconnect {id}`, and `GET /api/account/me` with the cookie, which adds the
account's `connections`. Storage is one JSON file in `LSUITE_DATA_DIR` (0600, written atomically;
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
