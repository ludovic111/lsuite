# The lsuite agent harness

Decided by the owner on 2026-10-07: every lsuite app, and the suite as a whole, has a **real agent
harness**: an agent working in lsuite knows the trade, knows the app, sees what it made and checks
it before it says it's done, so it does the job better there than with any other software. The
harness is the same whether the agent is the app's built-in one, the lsuite app's suite agent, or
an outside agent (Claude Code, Codex) connected through the app's MCP server.

## The eight parts

1. **An expert brief.** The system prompt of the app's agent (and the MCP server's
   `instructions`): who the agent is in this trade, the document's mental model, which commands do
   the common jobs, the trade's quality bar (levels and loudness for music, pacing and cuts for
   video, type and grids for design, structure and formatting for office documents, tests and
   diffs for code), the usual mistakes, and the finish routine (part 5). Concrete, with short
   examples; 800 to 1,500 words; generated from one source so the built-in agent and MCP agree.
2. **Skills: playbooks for the trade's jobs.** 8 to 15 per app, each a short markdown recipe:
   when to use it, the steps with the exact commands, the checks that prove it worked. Built in
   (`harness/skills/*.md` in the app's control crate). The agent sees their index in its brief and
   loads one with `harness.skill {name}` (`harness.skills` lists them); over MCP each skill is
   also a prompt and a resource (`<app>://skills/<name>`).
3. **Live context.** Before every model step (not only every request), a compact summary of the
   document: what it holds, what is selected, where the playhead or page is, what the person
   changed since the last step. The full state stays one command away (`*.overview`).
4. **Eyes and ears.** The agent can look at and measure what it made, and pictures reach the model
   as images (built-in agent and MCP alike): a frame or range of the timeline (kimchi), a page or
   layer (nori), a page, a slide or a sheet range (folio), a bar range's waveform, spectrum and
   piano roll with loudness, true peak and clipping numbers (ryolune), the running app or a web
   preview (zenith). `harness.look` is the app's best picture of the current work.
5. **A finish routine.** Before it says it's done the agent looks and measures, compares the
   result with the request, fixes what's off (up to three passes), then reports what changed in a
   few lines. Written into the brief and the skills; checked by the evals.
6. **One undo per agent turn.** A checkpoint before the agent's first edit; the person can revert
   the whole turn, and sees the list of changes.
7. **Evals.** `evals/` in each app: 10 or more scripted jobs of the trade (from a blank document
   or a fixture), run headless through the CLI with a real model (the Claude Code provider, so no
   key is needed on a developer's machine; or `ANTHROPIC_API_KEY`), each scored by automatic
   checks on the resulting document (structure, numbers, rendered pictures) and recorded in
   `evals/RESULTS.md` with the date, model and pass rate. Run before each release; a harness change
   that lowers the pass rate doesn't ship.
8. **The suite harness.** In the lsuite app, the **lsuite agent** takes a job that spans apps
   ("score this cut", "turn this report into a deck and a poster"), plans it, drives each app
   through its MCP server with that app's skills, moves files between apps (STANDARD.md section 4)
   and checks the result in each. zenith hands its Claude Code and Codex threads the lsuite apps'
   MCP servers **with** their briefs and skills (appended instructions), so a coding agent knows
   how to use them.

## Commands (same names everywhere)

| Command | Does |
| --- | --- |
| `harness.brief` | The expert brief (markdown). |
| `harness.skills` | `[{name, title, when}]`. |
| `harness.skill {name}` | One skill's markdown. |
| `harness.context` | The live context of part 3 (what the agent gets before each step). |
| `harness.look {…}` | The app's best picture of the current work (an image the model sees), with its numbers. |

## Status (2026-10-07)

Being built in every app and in the lsuite app (launcher 0.2.0). Each app's CLAUDE.md "lsuite"
section tracks its parts; `evals/RESULTS.md` holds its scores.
