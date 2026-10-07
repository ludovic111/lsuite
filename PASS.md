# lsuite Pass: everything extra, in one plan

Decided by the owner on 2026-10-07: the paid subscription is **lsuite Pass**. The apps stay free
and open source, with every feature; the Pass is the one thing lsuite sells, and it is optional.
It holds three things, one lsuite account for all of them:

| Part | What it is | Contract |
| --- | --- | --- |
| **lsuite AI** | The agents in every app with nothing to set up: Claude through lsuite's Anthropic-compatible endpoint, a monthly allowance of credits. The apps' provider keeps the name "lsuite AI". | [AI.md](AI.md) |
| **lsuite Cloud** | Storage for your files, managed and synced from the lsuite launcher. | [CLOUD.md](CLOUD.md) |
| **lsuite Marketplace** | Plugins for the five apps made by the people who use them and by lsuite, every version reviewed. Installing comes with a paid plan; anyone with an account can publish. | [MARKETPLACE.md](MARKETPLACE.md) |

Bringing your own model (Claude Code, Codex, API keys, Ollama) stays free and is never pushed
aside; documents stay plain files on disk; no telemetry.

**For now this is a demo.** No payment is taken: checkout says so and activates the plan at once,
lsuite AI answers with a demo message, cloud storage is capped small and creators aren't paid.
Production activation (Stripe, Anthropic, email, the object store) is in AI.md and CLOUD.md.

## Plans

Monthly prices approved by the owner on 2026-10-07, in USD.

| Plan | Price | lsuite AI models | Credits a month | lsuite Cloud | lsuite Marketplace |
| --- | --- | --- | --- | --- | --- |
| **Free** | 0 | bring your own provider | — | — (files already there: list, download, delete) | browse and publish |
| **Plus** | $12 / month | Claude Sonnet, Claude Haiku | 1,000 | 50 GB | install |
| **Pro** | $29 / month | + Claude Opus | 4,000 | 250 GB | install |
| **Studio** | $79 / month | everything, priority | 12,000 | 1 TB | install |

Demo caps: 100 MB of cloud storage per account (CLOUD.md), marketplace bundles up to 15 MB each
and 80 MB for every plugin together (MARKETPLACE.md): the site's Railway volume is 500 MB and holds
the accounts, the cloud and the marketplace.

`GET /api/ai/plans` is the source of truth the apps, the launcher and the site read:
`product: {name: "lsuite Pass", parts: ["lsuite AI", "lsuite Cloud", "lsuite Marketplace"], page:
"/pass"}` and, per plan, `credits`, `models`, `families`, `storage`, `storageLabel`, `marketplace`
(true on paid plans) besides the fields AI.md lists. The API paths keep their names
(`/api/ai/…`, `/api/account/…`, `/api/cloud/…`, `/api/marketplace/…`).

## On the site

- `/pass` (`pass/index.html`): the three parts, how lsuite AI works, the plans
  (`<!-- include:plans -->`, from `plansHtml()` in `server.js`), bring your own, the API, questions.
  `/ai` is a 301 to `/pass` and stays out of the sitemap.
- `/marketplace` (`marketplace/index.html`): the approved plugins, filtered by app with
  `?app=<app>` (`<!-- include:marketplace -->`), how to publish, safety.
- `/account`: the Pass (plan and allowance), lsuite Cloud's usage, **Your plugins** (each
  submission's review) and, for admins (`LSUITE_ADMIN_EMAILS`), **Review** (approve or reject with a
  note). `/account/checkout` and `/account/connect` say lsuite Pass.
- Nav: **Pass** and **Marketplace** after the apps; the footer, the home page and each app's page
  link to them. Where the text is about the agents, models or credits, it says lsuite AI.
